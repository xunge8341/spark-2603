use std::net::TcpListener;

use mio::net::TcpStream as MioTcpStream;
use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::io::AcceptDecision;
use spark_transport::policy::AcceptPolicy;
use spark_transport::reactor::Interest;
use spark_transport::KernelError;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics};

use crate::io::MioIo;
use crate::tcp::TcpChannel;
use crate::MioBridge;

/// Accept as many connections as allowed by `cfg.max_accept_per_tick`.
///
/// Why this exists:
/// - keep `spawn_tcp_dataplane` readable (single responsibility);
/// - make nonblocking accept behavior testable (Windows/Unix differences).
///
/// Returns number of accepted connections.
pub(crate) fn accept_tick<A>(
    listener: &TcpListener,
    bridge: &mut MioBridge<A>,
    cfg: &DataPlaneConfig,
    metrics: &std::sync::Arc<DataPlaneMetrics>,
) -> Result<usize, KernelError>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    let policy = AcceptPolicy;
    let mut accepted = 0usize;

    for _ in 0..cfg.max_accept_per_tick {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let _ = stream.set_nodelay(true);
                let mio_stream = MioTcpStream::from_std(stream);

                let chan_id = match bridge.alloc_chan_id() {
                    Ok(id) => id,
                    Err(_) => {
                        // At capacity: reject and close immediately (drop stream).
                        metrics.accept_rejected_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                };
                let mut tcp = match TcpChannel::new(chan_id, mio_stream, cfg.max_frame_hint()) {
                    Ok(ch) => ch,
                    Err(_) => {
                        metrics.accept_errors_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        metrics.accept_errors_fatal_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        bridge.__release_uninstalled_chan_id(chan_id);
                        return Err(KernelError::Internal(spark_transport::error_codes::ERR_MIO_ACCEPT_SETUP_FAILED));
                    }
                };

                // 初始只订阅 READ。
                if bridge
                    .reactor_mut()
                    .register_tcp(chan_id, tcp.stream_mut(), Interest::READ)
                    .is_err()
                {
                    metrics.accept_errors_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    metrics.accept_errors_fatal_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    bridge.__release_uninstalled_chan_id(chan_id);
                    return Err(KernelError::Internal(spark_transport::error_codes::ERR_MIO_REGISTER_FAILED));
                }
                if bridge.install_channel(chan_id, MioIo::Tcp(tcp)).is_err() {
                    // Should be extremely rare (slot should be free), but keep it safe and leak-free.
                    metrics.accept_rejected_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Best-effort: remove mio registration bookkeeping and return the id to the free list.
                    bridge.reactor_mut().forget(chan_id);
                    bridge.__release_uninstalled_chan_id(chan_id);
                    continue;
                }
                metrics.accepted_total.fetch_add(
                    1,
                    std::sync::atomic::Ordering::Relaxed,
                );
                accepted += 1;
            }
            Err(e) => {
                match policy.decide(&e) {
                    AcceptDecision::Continue => {
                        // Interrupted is not an error; transient abort/reset is.
                        if e.kind() != std::io::ErrorKind::Interrupted {
                            metrics.accept_errors_total.fetch_add(
                                1,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                            metrics.accept_errors_transient_total.fetch_add(
                                1,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                        }
                        continue;
                    }
                    AcceptDecision::Stop => break,
                    AcceptDecision::Fatal(k) => {
                        metrics.accept_errors_total.fetch_add(
                            1,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        metrics.accept_errors_fatal_total.fetch_add(
                            1,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        return Err(k);
                    }
                }
            }
        }
    }

    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::accept_tick;
    use crate::evidence::build_default_evidence;
    use crate::{MioBridge, MioReactor, QueueExecutor};
    use spark_buffer::Bytes;
    use spark_core::context::Context;
    use spark_core::service::Service;
    use spark_transport::async_bridge::FrameDecoderProfile;
    use spark_transport::{Budget, DataPlaneConfig, DataPlaneMetrics, KernelError};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct NoopService;

    impl Service<Bytes> for NoopService {
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
    fn accept_tick_respects_cap_and_wouldblock() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("addr");

        let cfg = DataPlaneConfig {
            max_channels: 64,
            framing: FrameDecoderProfile::line(4096),
            max_accept_per_tick: 1,
            budget: Budget {
                max_events: 64,
                max_nanos: 0,
            },
            ..Default::default()
        };

        let metrics = Arc::new(DataPlaneMetrics::default());
        let evidence = build_default_evidence(metrics.clone(), false);
        let reactor = MioReactor::new().expect("reactor");
        let executor = QueueExecutor::new();
        let app = Arc::new(NoopService);
        let mut bridge: MioBridge<NoopService> = spark_transport::AsyncBridge::new_with_dataplane_config(
            reactor,
            executor,
            app,
            &cfg,
            metrics.clone(),
            evidence,
        );

        // Prepare multiple pending connections.
        let _c1 = TcpStream::connect(addr).expect("connect1");
        let _c2 = TcpStream::connect(addr).expect("connect2");
        let _c3 = TcpStream::connect(addr).expect("connect3");

        let mut total = 0usize;
        let deadline = Instant::now() + Duration::from_secs(2);
        while total < 3 && Instant::now() < deadline {
            let n = accept_tick(&listener, &mut bridge, &cfg, &metrics).expect("accept_tick");
            assert!(n <= cfg.max_accept_per_tick);
            total += n;
            if n == 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        assert_eq!(total, 3, "expected to accept all pending connections");
        assert_eq!(
            metrics.accepted_total.load(Ordering::Relaxed),
            3,
            "accepted_total should match"
        );
        assert_eq!(
            metrics.active_connections.load(Ordering::Relaxed),
            3,
            "active_connections should match"
        );

        // No more pending connections -> WouldBlock -> accept_tick returns 0.
        let n = accept_tick(&listener, &mut bridge, &cfg, &metrics).expect("accept_tick");
        assert_eq!(n, 0);
    }
}
