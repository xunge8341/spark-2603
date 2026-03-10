//! Std-only HTTP/1.1 management-plane adapter.
//!
//! This is intentionally small and dependency-light. It is **not** a general-purpose
//! HTTP server; it exists to provide a pragmatic, self-contained control-plane for
//! Spark artifacts.

use crate::http1::EmberState;
use crate::util::block_on;

use spark_codec::prelude::*;
use spark_codec_http::http1::{write_response, Http1DecodeError, Http1HeadDecoder, RequestHead};
use spark_host::config::ServerConfig;
use spark_host::router::{MgmtRequest, MgmtResponse, MgmtState, RouteKind, RouteTable};

use std::io::Read;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

/// Kestrel-like server core (management plane).
///
/// Note: this is a **control-plane** server, not the dataplane (high-performance I/O).
pub struct Server {
    config: ServerConfig,
    routes: Arc<RouteTable>,
    state: Arc<EmberState>,
    inflight: Arc<AtomicUsize>,
}

impl Server {
    pub fn new(config: ServerConfig, routes: Arc<RouteTable>, state: Arc<EmberState>) -> Self {
        Self {
            config,
            routes,
            state,
            inflight: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[inline]
    pub fn state(&self) -> Arc<EmberState> {
        Arc::clone(&self.state)
    }

    /// Blocking serve loop.
    ///
    /// - One thread per connection (mgmt QPS is low; simplicity wins).
    /// - Minimal HTTP/1.1 (Connection: close).
    pub fn serve(self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.config.mgmt_addr)?;
        self.serve_on(listener)
    }

    /// Serve on a pre-bound listener (useful for tests and advanced hosting).
    pub fn serve_on(self, listener: TcpListener) -> std::io::Result<()> {
        listener.set_nonblocking(true)?;

        let routes = Arc::clone(&self.routes);
        let state = self.state();
        let inflight = Arc::clone(&self.inflight);

        let max_req = self.config.effective_max_request_bytes();
        let max_head = self.config.max_head_bytes;
        let max_headers = self.config.max_headers;
        let max_body = self.config.max_body_bytes;
        let max_inflight = self.config.mgmt_max_inflight;

        loop {
            if state.is_draining() {
                break;
            }

            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    // Coarse isolation: bound concurrent connections.
                    let prev = inflight.fetch_add(1, Ordering::AcqRel);
                    if prev >= max_inflight {
                        inflight.fetch_sub(1, Ordering::Release);
                        let _ = write_response(&mut stream, 503, "text/plain; charset=utf-8", b"Busy");
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }

                    let routes = Arc::clone(&routes);
                    let state = Arc::clone(&state);
                    let inflight = Arc::clone(&inflight);
                    thread::spawn(move || {
                        let _guard = InflightGuard { inflight };
                        let _ = handle_conn(stream, routes, state, max_req, max_head, max_headers, max_body);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => {
                    eprintln!("[ember] accept error: {e}");
                    thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }

        Ok(())
    }

    /// Convenience for invoking the management router in tests.
    pub fn handle_mgmt_sync(&self, kind: &str, path: &str, body: Vec<u8>) -> MgmtResponse {
        if let Some(entry) = self.routes.lookup(kind, path) {
            block_on((entry.handler)(MgmtRequest {
                kind: RouteKind::from(kind),
                path: path.to_string().into_boxed_str(),
                body,
                state: self.state() as Arc<dyn MgmtState>,
            }))
        } else {
            MgmtResponse::status(404, "Not Found")
        }
    }
}

struct InflightGuard {
    inflight: Arc<AtomicUsize>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::Release);
    }
}

fn handle_conn(
    mut stream: TcpStream,
    routes: Arc<RouteTable>,
    state: Arc<EmberState>,
    max_request_bytes: usize,
    max_head_bytes: usize,
    max_headers: usize,
    max_body_bytes: usize,
) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 1024];

    let mut decoder = Http1HeadDecoder::with_limits(max_head_bytes, max_headers);
    let head: RequestHead = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > max_request_bytes {
            write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
            return Ok(());
        }

        let mut ctx = spark_core::context::Context::default();
        match decoder.decode(&mut ctx, &buf) {
            Ok(DecodeOutcome::NeedMore) => continue,
            Ok(DecodeOutcome::Message { consumed, message }) => {
                let content_len = message.content_length();
                if content_len > max_body_bytes {
                    write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
                    return Ok(());
                }
                if consumed.saturating_add(content_len) > max_request_bytes {
                    write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
                    return Ok(());
                }
                // Remove the consumed head bytes; keep any already-read body bytes.
                buf.drain(..consumed);
                break message;
            }
            Err(Http1DecodeError::HeadTooLarge) => {
                write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
                return Ok(());
            }
            Err(_) => {
                write_response(&mut stream, 400, "text/plain; charset=utf-8", b"Bad Request")?;
                return Ok(());
            }
        }
    };

    let content_len = head.content_length();

    // `buf` contains already-read body bytes.
    if content_len > max_body_bytes {
        write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
        return Ok(());
    }
    if max_head_bytes.saturating_add(content_len) > max_request_bytes {
        write_response(&mut stream, 413, "text/plain; charset=utf-8", b"Request Too Large")?;
        return Ok(());
    }

    let mut body = Vec::with_capacity(content_len);
    if !buf.is_empty() {
        if buf.len() > content_len {
            body.extend_from_slice(&buf[..content_len]);
        } else {
            body.extend_from_slice(&buf);
        }
    }

    while body.len() < content_len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        let remaining = content_len - body.len();
        body.extend_from_slice(&tmp[..n.min(remaining)]);
    }

    let kind_ref = head.method.as_ref();
    let path_ref = head.path.as_ref();
    let resp = if let Some(entry) = routes.lookup(kind_ref, path_ref) {
        block_on((entry.handler)(MgmtRequest {
            kind: RouteKind::from(kind_ref),
            path: head.path,
            body,
            state: state as Arc<dyn MgmtState>,
        }))
    } else {
        MgmtResponse::status(404, "Not Found")
    };

    write_response(&mut stream, resp.status, resp.content_type, &resp.body)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}
