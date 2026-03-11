use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_ember::server::{EmberState, Server};
use spark_host::builder::HostBuilder;
use spark_host::router::RouteTable;
use spark_transport::KernelError;
use spark_uci::names::metrics as mn;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

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

#[test]
fn mgmt_metrics_smoke_over_tcp() {
    // Build a host spec with default diagnostics (/healthz, /readyz, /metrics, /drain).
    let spec = HostBuilder::new()
        .use_default_diagnostics()
        .pipeline(|pb| pb.service(Noop))
        .build()
        .expect("host build");

    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());

    let state = Arc::new(EmberState::new());

    // Bind to an ephemeral port for stable CI.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let server_for_sync = Server::new(spec.config.clone(), Arc::clone(&routes), Arc::clone(&state));
    let server = Server::new(spec.config.clone(), routes, Arc::clone(&state));

    let t = thread::spawn(move || {
        let _ = server.serve_on(listener);
    });

    // Issue a single GET /metrics request.
    let mut stream = TcpStream::connect(addr).expect("connect");
    let req = b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(req).expect("write");

    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");

    let accepted = format!("spark_dp_{}", mn::ACCEPTED_TOTAL);
    let active = format!("spark_dp_{}", mn::ACTIVE_CONNECTIONS);
    let read_bytes = format!("spark_dp_{}", mn::READ_BYTES_TOTAL);
    let write_bytes = format!("spark_dp_{}", mn::WRITE_BYTES_TOTAL);

    assert!(
        buf.contains(&accepted),
        "missing {} in /metrics output",
        accepted
    );
    assert!(
        buf.contains(&active),
        "missing {} in /metrics output",
        active
    );
    assert!(
        buf.contains(&read_bytes),
        "missing {} in /metrics output",
        read_bytes
    );
    assert!(
        buf.contains(&write_bytes),
        "missing {} in /metrics output",
        write_bytes
    );

    state.set_listener_ready(false);
    let ready_listener = server_for_sync.handle_mgmt_sync("GET", "/readyz", Vec::new());
    assert_eq!(ready_listener.status, 503);

    state.set_listener_ready(true);
    state.set_dependencies_ready(false);
    let ready_deps = server_for_sync.handle_mgmt_sync("GET", "/readyz", Vec::new());
    assert_eq!(ready_deps.status, 503);
    state.set_dependencies_ready(true);

    state.set_draining(true);
    let health_while_draining = server_for_sync.handle_mgmt_sync("GET", "/healthz", Vec::new());
    assert_eq!(health_while_draining.status, 200);

    // Stop server.
    state.set_draining(true);
    let ready = server_for_sync.handle_mgmt_sync("GET", "/readyz", Vec::new());
    assert_eq!(ready.status, 503);
    let _ = t.join();

    // Also sanity-check drain route toggles readiness.
    let resp = server_for_sync.handle_mgmt_sync("POST", "/drain", Vec::new());
    assert!(matches!(resp.status, 200 | 202));
}

#[test]
fn mgmt_metrics_expose_overload_counters() {
    let spec = HostBuilder::new()
        .use_default_diagnostics()
        .pipeline(|pb| pb.service(Noop))
        .build()
        .expect("host build");

    spec.metrics
        .overload_reject_total
        .store(1, std::sync::atomic::Ordering::Relaxed);
    spec.metrics
        .app_queue_high_watermark
        .store(3, std::sync::atomic::Ordering::Relaxed);

    let routes = Arc::new(RouteTable::new());
    routes.replace_all(spec.mgmt.clone());
    let state = Arc::new(EmberState::new());
    let server = Server::new(spec.config.clone(), routes, state);

    let resp = server.handle_mgmt_sync("GET", "/metrics", Vec::new());
    assert_eq!(resp.status, 200);
    let body = String::from_utf8(resp.body).unwrap_or_default();

    let overload_reject = format!("spark_dp_{} 1", mn::OVERLOAD_REJECT_TOTAL);
    assert!(
        body.contains(&overload_reject),
        "missing overload counter in /metrics: {}",
        overload_reject
    );

    let queue_hwm = format!("spark_dp_{} 3", mn::APP_QUEUE_HIGH_WATERMARK);
    assert!(
        body.contains(&queue_hwm),
        "missing queue watermark metric in /metrics: {}",
        queue_hwm
    );
}
