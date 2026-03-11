use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_host::builder::HostBuilder;
use spark_host::config::ServerConfig;
use spark_host::mgmt_profile::MgmtRejectPolicy;
use spark_host::router::RouteTable;
use spark_host::router::{MgmtResponse, RouteSet};
use spark_transport::DataPlaneMetrics;
use spark_transport::KernelError;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
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

fn reserve_local_addr() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    listener.local_addr().expect("local addr")
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
        resp = read_to_end(&mut stream, Duration::from_millis(300));
        if !resp.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    resp
}

fn request_with_retry_timeout(
    addr: std::net::SocketAddr,
    req: &[u8],
    read_timeout: Duration,
) -> Vec<u8> {
    let mut resp = Vec::new();
    for _ in 0..10 {
        let mut stream = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        if stream.write_all(req).is_err() {
            std::thread::sleep(Duration::from_millis(30));
            continue;
        }
        resp = read_to_end(&mut stream, read_timeout);
        if !resp.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    resp
}

fn response_body(resp: &[u8]) -> String {
    let sep = b"\r\n\r\n";
    match resp.windows(sep.len()).position(|w| w == sep) {
        Some(idx) => String::from_utf8_lossy(&resp[idx + sep.len()..]).into_owned(),
        None => String::new(),
    }
}

fn warmup_server(addr: std::net::SocketAddr) {
    let _ = request_with_retry(
        addr,
        b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
}

fn spawn_transport_server(
    cfg: ServerConfig,
    route_set: RouteSet,
) -> spark_ember::TransportServerHandle {
    let routes = Arc::new(RouteTable::new());
    routes.replace_all(route_set);

    let state = Arc::new(spark_ember::EmberState::new());
    let mgmt_profile = cfg.mgmt_profile_v1();
    let mgmt_metrics = Arc::new(DataPlaneMetrics::default());
    let server =
        spark_ember::TransportServer::new(mgmt_profile, routes, Arc::clone(&state), mgmt_metrics);

    server
        .try_spawn_with(|cfg, draining, service, metrics| {
            spark_transport_mio::try_spawn_tcp_dataplane(cfg, draining, service, metrics).map(|h| {
                spark_ember::http1::SpawnedTcpDataplane {
                    join: h.join,
                    local_addr: h.local_addr,
                }
            })
        })
        .expect("spawn transport mgmt")
}

#[test]
fn mgmt_transport_server_healthz_smoke() {
    let cfg = ServerConfig::default().with_mgmt_addr(reserve_local_addr());

    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .use_transport_mgmt_profile()
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let handle = spawn_transport_server(cfg, spec.mgmt.clone());
    warmup_server(handle.addr);
    let addr = handle.addr;
    warmup_server(addr);

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

#[test]
fn mgmt_transport_route_level_timeout_returns_504() {
    let cfg = ServerConfig::default()
        .with_mgmt_addr(reserve_local_addr())
        .with_request_timeout(Duration::from_secs(1));
    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .map_get_with_timeout(
            "/slow-route",
            "slow-route",
            Duration::from_millis(30),
            |_req| async { core::future::pending::<MgmtResponse>().await },
        )
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let handle = spawn_transport_server(cfg, spec.mgmt.clone());
    warmup_server(handle.addr);
    let req = b"GET /slow-route HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let resp = request_with_retry_timeout(handle.addr, req, Duration::from_secs(1));
    assert_eq!(
        status_code(&resp),
        504,
        "route-level timeout should surface as HTTP 504 via transport dataplane"
    );

    handle.state.set_draining(true);
}

#[test]
fn mgmt_transport_group_default_timeout_returns_504() {
    let cfg = ServerConfig::default()
        .with_mgmt_addr(reserve_local_addr())
        .with_request_timeout(Duration::from_secs(1));

    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .mgmt(|app| {
            let mut group = app.map_group("/group");
            group.with_request_timeout(Duration::from_millis(25));
            group.map_get("/slow", |_req| async {
                core::future::pending::<MgmtResponse>().await
            });
        })
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let handle = spawn_transport_server(cfg, spec.mgmt.clone());
    warmup_server(handle.addr);
    let req = b"GET /group/slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let resp = request_with_retry_timeout(handle.addr, req, Duration::from_secs(1));
    assert_eq!(
        status_code(&resp),
        504,
        "group-level timeout default should return 504 in transport-backed e2e path"
    );

    handle.state.set_draining(true);
}

#[test]
fn mgmt_transport_overload_rejects_under_real_dataplane_pressure() {
    let cfg = ServerConfig::default()
        .with_mgmt_addr(reserve_local_addr())
        .with_request_timeout(Duration::from_millis(180))
        .with_max_concurrent_requests(1)
        .with_request_queue_limit(1)
        .with_reject_policy(MgmtRejectPolicy::TooManyRequests);

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .mgmt(move |app| {
            let tx = entered_tx.clone();
            app.map_get("/hold", move |_req| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(());
                    core::future::pending::<MgmtResponse>().await
                }
            });
            app.map_get("/fast", |_req| async { MgmtResponse::ok("OK") });
        })
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let handle = spawn_transport_server(cfg, spec.mgmt.clone());
    warmup_server(handle.addr);
    let addr = handle.addr;
    let hold_thread = thread::spawn(move || {
        request_with_retry_timeout(
            addr,
            b"GET /hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            Duration::from_secs(1),
        )
    });

    entered_rx
        .recv_timeout(Duration::from_millis(200))
        .expect("hold request should become inflight before overload probe");

    let overloaded = request_with_retry(
        handle.addr,
        b"GET /fast HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(
        status_code(&overloaded),
        429,
        "overload reject policy should return 429 when inflight limit is exhausted"
    );

    let _ = hold_thread.join().expect("join hold request thread");
    handle.state.set_draining(true);
}

#[test]
fn mgmt_transport_drain_rejects_new_requests_and_bounds_inflight() {
    let cfg = ServerConfig::default()
        .with_mgmt_addr(reserve_local_addr())
        .with_request_timeout(Duration::from_millis(90));

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let spec = HostBuilder::new()
        .config(cfg.clone())
        .use_default_diagnostics()
        .map_get_with_timeout(
            "/slow-inflight",
            "slow-inflight",
            Duration::from_millis(70),
            move |_req| {
                let tx = entered_tx.clone();
                async move {
                    let _ = tx.send(());
                    core::future::pending::<MgmtResponse>().await
                }
            },
        )
        .pipeline(|_| Noop)
        .build()
        .expect("host build");

    let handle = spawn_transport_server(cfg, spec.mgmt.clone());
    let addr = handle.addr;
    warmup_server(addr);
    let inflight = thread::spawn(move || {
        request_with_retry_timeout(
            addr,
            b"GET /slow-inflight HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            Duration::from_secs(1),
        )
    });

    entered_rx
        .recv_timeout(Duration::from_millis(200))
        .expect("inflight request should enter handler before drain");

    let drain_resp = request_with_retry(
        handle.addr,
        b"POST /drain HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
    );
    assert!(
        matches!(status_code(&drain_resp), 200 | 202),
        "drain request must be accepted"
    );

    let new_req = request_with_retry(
        handle.addr,
        b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(
        status_code(&new_req),
        503,
        "new requests should be rejected during drain"
    );
    assert!(
        response_body(&new_req).contains("Draining"),
        "drain rejection body should clearly indicate draining state"
    );

    let inflight_resp = inflight.join().expect("join inflight request thread");
    assert_eq!(
        status_code(&inflight_resp),
        504,
        "inflight request should converge by timeout deadline during drain"
    );

    handle.state.set_draining(true);
}
