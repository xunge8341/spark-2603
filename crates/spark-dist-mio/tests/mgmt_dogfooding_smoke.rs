use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_host::builder::HostBuilder;
use spark_host::config::ServerConfig;
use spark_host::router::RouteTable;
use spark_transport::KernelError;
use spark_transport::DataPlaneMetrics;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
struct Noop;

impl Service<Bytes> for Noop {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

fn read_to_end(stream: &mut TcpStream, timeout: Duration) -> Vec<u8> {
    stream
        .set_read_timeout(Some(timeout))
        .expect("set read timeout");
    let mut out = Vec::<u8>::with_capacity(4096);
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Treat timeout as end-of-stream for this smoke test.
                break;
            }
            Err(e) => panic!("read failed: {e:?}"),
        }
    }
    out
}

/// Dogfooding gate (P0): mgmt-plane must run on top of `spark-transport` and be reachable.
///
/// This smoke test uses an ephemeral port (bind to 0) and relies on the spawn API returning
/// the actual bound address.
#[test]
fn mgmt_transport_server_healthz_smoke() {
    // Bind to an ephemeral port for CI reliability.
    let cfg = ServerConfig::default().with_mgmt_addr("127.0.0.1:0".parse().expect("addr"));

    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .use_transport_mgmt_profile()
        .pipeline(|_| Noop)
        .build().expect("host build");

    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());

    let state = Arc::new(spark_ember::EmberState::new());
    let mgmt_profile = spec.config.mgmt_profile_v1();
    let mgmt_metrics = Arc::new(DataPlaneMetrics::default());
    let server = spark_ember::TransportServer::new(mgmt_profile, routes, Arc::clone(&state), mgmt_metrics);

    let handle = server
        .try_spawn_with(|cfg, draining, service, metrics| {
            spark_transport_mio::try_spawn_tcp_dataplane(cfg, draining, service, metrics).map(|h| {
                spark_ember::http1::SpawnedTcpDataplane {
                    join: h.join,
                    local_addr: h.local_addr,
                }
            })
        })
        .expect("spawn transport mgmt");
    let addr = handle.addr;

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    // Minimal HTTP/1 request.
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write request");

    let resp = read_to_end(&mut stream, Duration::from_secs(2));
    let text = String::from_utf8_lossy(&resp);
    assert!(text.contains("200"), "expected 200 response, got: {text}");
    assert!(text.contains("OK"), "expected body 'OK', got: {text}");

    // Shutdown the mgmt dataplane.
    handle.state.set_draining(true);
    handle.join.join().expect("join mgmt dataplane");
}
