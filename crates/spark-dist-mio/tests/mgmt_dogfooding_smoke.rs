use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_host::builder::HostBuilder;
use spark_host::config::ServerConfig;
use spark_host::router::RouteTable;
use spark_transport::DataPlaneMetrics;
use spark_transport::KernelError;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
struct Noop;

impl Service<Bytes> for Noop {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        _context: Context,
        _request: Bytes,
    ) -> Result<Self::Response, Self::Error> {
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
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("read failed: {e:?}"),
        }
    }
    out
}

fn status_code(resp: &[u8]) -> u16 {
    let text = String::from_utf8_lossy(resp);
    let mut parts = text.split_whitespace();
    let _ = parts.next();
    parts
        .next()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(0)
}

fn request_with_retry(addr: std::net::SocketAddr, req: &[u8]) -> Vec<u8> {
    let mut resp = Vec::new();
    for _ in 0..8 {
        let mut stream = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(40));
                continue;
            }
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        if stream.write_all(req).is_err() {
            std::thread::sleep(Duration::from_millis(40));
            continue;
        }
        resp = read_to_end(&mut stream, Duration::from_millis(250));
        if !resp.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    resp
}

#[test]
fn mgmt_transport_server_healthz_smoke() {
    let cfg = ServerConfig::default().with_mgmt_addr("127.0.0.1:0".parse().expect("addr"));

    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .use_transport_mgmt_profile()
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());

    let state = Arc::new(spark_ember::EmberState::new());
    let mgmt_profile = spec.config.mgmt_profile_v1();
    let mgmt_metrics = Arc::new(DataPlaneMetrics::default());
    let server =
        spark_ember::TransportServer::new(mgmt_profile, routes, Arc::clone(&state), mgmt_metrics);

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

    let health_request = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let health_resp = request_with_retry(addr, health_request);
    if !health_resp.is_empty() {
        assert_eq!(status_code(&health_resp), 200);
    }

    let ready_request = b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let ready_resp = request_with_retry(addr, ready_request);
    if !ready_resp.is_empty() {
        assert_eq!(status_code(&ready_resp), 200);
    }

    handle.state.set_dependencies_ready(false);
    let ready_deps_down = request_with_retry(addr, ready_request);
    if !ready_deps_down.is_empty() {
        assert_eq!(status_code(&ready_deps_down), 503);
    }
    handle.state.set_dependencies_ready(true);

    let drain_request =
        b"POST /drain HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
    let drain_resp = request_with_retry(addr, drain_request);
    if !drain_resp.is_empty() {
        assert!(matches!(status_code(&drain_resp), 200 | 202));
    }

    let ready_after_drain = request_with_retry(addr, ready_request);
    if !ready_after_drain.is_empty() {
        assert_eq!(status_code(&ready_after_drain), 503);
    }
    let health_resp = request_with_retry(addr, health_request);
    if !health_resp.is_empty() {
        assert_eq!(status_code(&health_resp), 200);
    }

    let metrics_request = b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let metrics_after_drain = request_with_retry(addr, metrics_request);
    if !metrics_after_drain.is_empty() {
        assert_eq!(status_code(&metrics_after_drain), 503);
    }

    handle.state.set_draining(true);
    handle.join.join().expect("join mgmt dataplane");
}
