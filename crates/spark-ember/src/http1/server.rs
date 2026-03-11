use crate::http1::EmberState;
use crate::util::block_on_until;

use spark_codec::prelude::*;
use spark_codec_http::http1::{write_response, Http1DecodeError, Http1HeadDecoder, RequestHead};
use spark_host::config::ServerConfig;
use spark_host::router::{MgmtRequest, MgmtResponse, MgmtState, RouteKind, RouteTable};

use std::io::Read;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

pub struct Server {
    config: ServerConfig,
    routes: Arc<RouteTable>,
    state: Arc<EmberState>,
    inflight: Arc<AtomicUsize>,
    queued: Arc<AtomicUsize>,
    overloaded: Arc<AtomicBool>,
}

impl Server {
    pub fn new(config: ServerConfig, routes: Arc<RouteTable>, state: Arc<EmberState>) -> Self {
        Self {
            config,
            routes,
            state,
            inflight: Arc::new(AtomicUsize::new(0)),
            queued: Arc::new(AtomicUsize::new(0)),
            overloaded: Arc::new(AtomicBool::new(false)),
        }
    }

    #[inline]
    pub fn state(&self) -> Arc<EmberState> {
        Arc::clone(&self.state)
    }

    pub fn serve(self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.config.mgmt.bind)?;
        self.serve_on(listener)
    }

    pub fn serve_on(self, listener: TcpListener) -> std::io::Result<()> {
        listener.set_nonblocking(true)?;

        let routes = Arc::clone(&self.routes);
        let state = self.state();
        state.set_listener_ready(true);
        state.set_accepting_new_requests(true);
        let inflight = Arc::clone(&self.inflight);
        let queued = Arc::clone(&self.queued);
        let overloaded = Arc::clone(&self.overloaded);

        let max_req = self.config.effective_max_request_bytes();
        let max_head = self.config.mgmt.http.max_head_bytes;
        let max_headers = self.config.mgmt.http.max_headers;
        let max_body = self.config.mgmt.http.max_body_bytes;
        let max_inflight = self.config.mgmt.overload.max_concurrent_requests;
        let queue_limit = self.config.mgmt.overload.queue_limit;

        loop {
            if state.is_draining() {
                state.set_accepting_new_requests(false);
                break;
            }

            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    if should_reject(&inflight, &queued, max_inflight, queue_limit) {
                        overloaded.store(true, Ordering::Release);
                        state.set_overloaded(true);
                        reject_overload(&self.config, &mut stream);
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }

                    overloaded.store(false, Ordering::Release);
                    state.set_overloaded(false);

                    queued.fetch_add(1, Ordering::AcqRel);
                    inflight.fetch_add(1, Ordering::AcqRel);
                    state.inc_active_requests();
                    let routes = Arc::clone(&routes);
                    let state = Arc::clone(&state);
                    let inflight = Arc::clone(&inflight);
                    let queued = Arc::clone(&queued);
                    let cfg = self.config.clone();
                    thread::spawn(move || {
                        queued.fetch_sub(1, Ordering::Release);
                        let _guard = InflightGuard {
                            inflight,
                            state: Arc::clone(&state),
                        };
                        let _ = handle_conn(
                            stream,
                            routes,
                            state,
                            &cfg,
                            max_req,
                            max_head,
                            max_headers,
                            max_body,
                        );
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_e) => {
                    thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }

        state.set_accepting_new_requests(false);
        state.set_listener_ready(false);

        let drain_deadline = Instant::now() + self.config.mgmt.request_timeouts.default_timeout;
        while self.inflight.load(Ordering::Acquire) > 0 && Instant::now() < drain_deadline {
            thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    pub fn handle_mgmt_sync(&self, kind: &str, path: &str, body: Vec<u8>) -> MgmtResponse {
        if let Some(entry) = self.routes.lookup(kind, path) {
            let timeout = entry
                .request_timeout
                .unwrap_or(self.config.mgmt.request_timeouts.default_timeout);
            let deadline = Instant::now() + timeout;
            let out = block_on_until(
                (entry.handler)(MgmtRequest {
                    kind: RouteKind::from(kind),
                    path: path.to_string().into_boxed_str(),
                    body,
                    state: self.state() as Arc<dyn MgmtState>,
                }),
                deadline,
            );
            match out {
                Some(resp) => resp,
                None => MgmtResponse::status(504, "Request Timeout"),
            }
        } else {
            MgmtResponse::status(404, "Not Found")
        }
    }
}

fn should_reject(
    inflight: &AtomicUsize,
    queued: &AtomicUsize,
    max_inflight: usize,
    queue_limit: usize,
) -> bool {
    let inflight_now = inflight.load(Ordering::Acquire);
    let queued_now = queued.load(Ordering::Acquire);
    inflight_now >= max_inflight || queued_now >= queue_limit
}

fn reject_overload(cfg: &ServerConfig, stream: &mut TcpStream) {
    match cfg.mgmt.overload.reject_policy {
        spark_host::mgmt_profile::MgmtRejectPolicy::ServiceUnavailable => {
            let _ = write_response(stream, 503, "text/plain; charset=utf-8", b"Busy");
        }
        spark_host::mgmt_profile::MgmtRejectPolicy::TooManyRequests => {
            let _ = write_response(stream, 429, "text/plain; charset=utf-8", b"Busy");
        }
        spark_host::mgmt_profile::MgmtRejectPolicy::CloseConnection => {}
    }
}

struct InflightGuard {
    inflight: Arc<AtomicUsize>,
    state: Arc<EmberState>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::Release);
        self.state.dec_active_requests();
    }
}

fn handle_conn(
    mut stream: TcpStream,
    routes: Arc<RouteTable>,
    state: Arc<EmberState>,
    cfg: &ServerConfig,
    max_request_bytes: usize,
    max_head_bytes: usize,
    max_headers: usize,
    max_body_bytes: usize,
) -> std::io::Result<()> {
    let read_timeout = cfg
        .mgmt
        .connection_timeouts
        .read_timeout
        .min(cfg.mgmt.connection_timeouts.idle_timeout);
    let write_timeout = cfg
        .mgmt
        .connection_timeouts
        .write_timeout
        .min(cfg.mgmt.connection_timeouts.idle_timeout);
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(write_timeout))?;

    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 1024];

    let mut decoder = Http1HeadDecoder::with_limits(max_head_bytes, max_headers);
    let headers_deadline = Instant::now() + cfg.mgmt.connection_timeouts.request_headers_timeout;
    let head: RequestHead = loop {
        if Instant::now() >= headers_deadline {
            write_response(
                &mut stream,
                408,
                "text/plain; charset=utf-8",
                b"Request Timeout",
            )?;
            return Ok(());
        }
        let n = match stream.read(&mut tmp) {
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                write_response(
                    &mut stream,
                    408,
                    "text/plain; charset=utf-8",
                    b"Request Timeout",
                )?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > max_request_bytes {
            write_response(
                &mut stream,
                413,
                "text/plain; charset=utf-8",
                b"Request Too Large",
            )?;
            return Ok(());
        }

        let mut ctx = spark_core::context::Context::default();
        match decoder.decode(&mut ctx, &buf) {
            Ok(DecodeOutcome::NeedMore) => continue,
            Ok(DecodeOutcome::Message { consumed, message }) => {
                let content_len = message.content_length();
                if content_len > max_body_bytes {
                    write_response(
                        &mut stream,
                        413,
                        "text/plain; charset=utf-8",
                        b"Request Too Large",
                    )?;
                    return Ok(());
                }
                if consumed.saturating_add(content_len) > max_request_bytes {
                    write_response(
                        &mut stream,
                        413,
                        "text/plain; charset=utf-8",
                        b"Request Too Large",
                    )?;
                    return Ok(());
                }
                buf.drain(..consumed);
                break message;
            }
            Err(Http1DecodeError::HeadTooLarge) => {
                write_response(
                    &mut stream,
                    413,
                    "text/plain; charset=utf-8",
                    b"Request Too Large",
                )?;
                return Ok(());
            }
            Err(_) => {
                write_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"Bad Request",
                )?;
                return Ok(());
            }
        }
    };

    let content_len = head.content_length();
    let mut body = Vec::with_capacity(content_len);
    if !buf.is_empty() {
        if buf.len() > content_len {
            body.extend_from_slice(&buf[..content_len]);
        } else {
            body.extend_from_slice(&buf);
        }
    }

    while body.len() < content_len {
        let n = match stream.read(&mut tmp) {
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                write_response(
                    &mut stream,
                    408,
                    "text/plain; charset=utf-8",
                    b"Request Timeout",
                )?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        let remaining = content_len - body.len();
        body.extend_from_slice(&tmp[..n.min(remaining)]);
    }

    let kind_ref = head.method.as_ref();
    let path_ref = head.path.as_ref();

    if (!state.is_accepting_new_requests() || state.is_draining()) && path_ref != "/healthz" {
        write_response(&mut stream, 503, "text/plain; charset=utf-8", b"Draining")?;
        return Ok(());
    }
    let resp = if let Some(entry) = routes.lookup(kind_ref, path_ref) {
        let timeout = entry
            .request_timeout
            .unwrap_or(cfg.mgmt.request_timeouts.default_timeout);
        let out = block_on_until(
            (entry.handler)(MgmtRequest {
                kind: RouteKind::from(kind_ref),
                path: head.path,
                body,
                state: state as Arc<dyn MgmtState>,
            }),
            Instant::now() + timeout,
        );
        match out {
            Some(resp) => resp,
            None => MgmtResponse::status(504, "Request Timeout"),
        }
    } else {
        MgmtResponse::status(404, "Not Found")
    };

    write_response(&mut stream, resp.status, resp.content_type, &resp.body)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Server;
    use spark_host::config::ServerConfig;
    use spark_host::router::{MgmtApp, RouteTable};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn spawn_server(
        config: ServerConfig,
        app: MgmtApp,
    ) -> (
        std::net::SocketAddr,
        Arc<crate::http1::EmberState>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|_| panic!("bind failed"));
        let addr = listener
            .local_addr()
            .unwrap_or_else(|_| panic!("local_addr failed"));
        let routes = Arc::new(RouteTable::new());
        routes.replace_all(app.into_routes());
        let state = Arc::new(crate::http1::EmberState::new());
        let state_clone = Arc::clone(&state);
        let server = Server::new(config, routes, state_clone);
        let h = thread::spawn(move || {
            let _ = server.serve_on(listener);
        });
        (addr, state, h)
    }

    fn read_status(mut stream: TcpStream) -> u16 {
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).unwrap_or(0);
        let txt = String::from_utf8_lossy(&buf[..n]);
        let mut it = txt.split_whitespace();
        let _ = it.next();
        it.next().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0)
    }

    #[test]
    fn slow_headers_timeout_returns_408() {
        let cfg = ServerConfig::default()
            .with_request_headers_timeout(Duration::from_millis(100))
            .with_connection_read_timeout(Duration::from_millis(80));
        let app = MgmtApp::new();
        let (addr, state, join) = spawn_server(cfg, app);
        thread::sleep(Duration::from_millis(30));

        let mut stream = TcpStream::connect(addr).unwrap_or_else(|_| panic!("connect failed"));
        let _ = stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: x");
        thread::sleep(Duration::from_millis(150));
        let _ = stream.write_all(b"\r\n\r\n");
        let status = read_status(stream);
        assert!(status >= 400);

        state.set_draining(true);
        let _ = join.join();
    }

    #[test]
    fn drain_rejects_new_requests() {
        let cfg = ServerConfig::default();
        let mut app = MgmtApp::new();
        app.map_get("/healthz", |_| async {
            spark_host::router::MgmtResponse::ok("OK")
        });
        let (addr, state, join) = spawn_server(cfg, app);
        thread::sleep(Duration::from_millis(30));

        state.set_draining(true);
        thread::sleep(Duration::from_millis(40));
        let conn = TcpStream::connect(addr);
        assert!(conn.is_err());
        let _ = join.join();
    }
}
