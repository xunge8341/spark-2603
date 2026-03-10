//! IOCP distribution dataplane smoke test (Windows).
//!
//! DECISION (BigStep-15, "将军赶路"):
//! - Multi-backend work must be validated by **dogfooding-style** end-to-end tests, not only unit contracts.
//! - Before a native IOCP completion driver lands, we still need a **Windows-only distribution path** that
//!   can be exercised in CI and locally. This test ensures the `spark-dist-iocp` route is a real, runnable
//!   dataplane (TCP accept + framing + writev flush + orderly close).
//! - The test payload is intentionally small and protocol-agnostic (line framing) to keep it stable.

#![cfg(windows)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};

struct Echo;

#[allow(async_fn_in_trait)]
impl Service<Bytes> for Echo {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(Some(request))
    }
}

fn read_line_with_deadline(mut s: &TcpStream, deadline: Instant) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    let mut one = [0u8; 1];
    while Instant::now() < deadline {
        match s.read(&mut one) {
            Ok(0) => break,
            Ok(1) => {
                buf.push(one[0]);
                if one[0] == b'\n' {
                    break;
                }
            }
            Ok(_) => unreachable!(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("read failed: {e}"),
        }
    }
    buf
}

#[test]
fn iocp_tcp_echo_smoke() {
    // DECISION:
    // This is a **line-framed** dataplane smoke. The client must send a real LF ("\n"), not a literal
    // backslash-n ("\\n"), otherwise the inbound Line decoder will never emit a frame and the echo
    // service will not run. Keeping this here prevents a class of "backend is broken" false alarms.

    let cfg = DataPlaneConfig::tcp("127.0.0.1:0".parse().unwrap()).normalized();
    assert!(cfg.validate().is_ok());

    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());
    let app = Arc::new(Echo);

    let h = spark_transport_iocp::try_spawn_tcp_dataplane(cfg, draining, app, metrics).expect("spawn");

    let mut c = TcpStream::connect(h.local_addr).expect("connect");
    c.set_nonblocking(true).ok();

    // Send a few line-framed messages.
    for i in 0..8u8 {
        // Per-message deadline keeps the test stable even if an earlier iteration stalls briefly.
        let deadline = Instant::now() + Duration::from_secs(5);

        let msg = format!("ping-{i}\n");
        c.write_all(msg.as_bytes()).expect("write");
        let got = read_line_with_deadline(&c, deadline);
        assert_eq!(got, msg.as_bytes());
    }
}
