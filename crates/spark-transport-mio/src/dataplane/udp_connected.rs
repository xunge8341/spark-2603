//! Connected-UDP dataplane (mio backend).

use crate::{MioBridge, MioReactor, QueueExecutor, UdpSocketChannel};

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::reactor::Interest;
use spark_transport::KernelError;
use spark_transport::{AsyncBridge, DataPlaneConfig, DataPlaneMetrics};

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Try-spawn a connected-UDP dataplane (mio backend).
///
/// This performs fallible socket setup in the caller thread (bind/connect),
/// then constructs the driver inside the spawned thread (which may be `!Send`).
///
/// A small startup handshake is used to keep this API fallible.
pub fn try_spawn_udp_dataplane<A>(
    cfg: DataPlaneConfig,
    remote: SocketAddr,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<thread::JoinHandle<()>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    let cfg = cfg.normalized();
    if let Err(e) = cfg.validate() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
    }
    let sock = mio::net::UdpSocket::bind(cfg.bind)?;
    sock.connect(remote)?;

    let (tx, rx) = std::sync::mpsc::channel::<std::io::Result<()>>();

    let handle = thread::spawn(move || {
        let reactor = match MioReactor::new() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        let executor = QueueExecutor::new();
        let evidence = crate::evidence::build_default_evidence(metrics.clone(), cfg.emit_evidence_log);

        let mut bridge: MioBridge<A> = AsyncBridge::new_with_dataplane_config(
            reactor,
            executor,
            app,
            &cfg,
            metrics.clone(),
            evidence,
        );

        let chan_id = match bridge.alloc_chan_id() {
            Ok(id) => id,
            Err(_) => {
                metrics.accept_rejected_total.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(Err(std::io::Error::other("dataplane capacity exhausted")));
                return;
            }
        };
        let mut udp = UdpSocketChannel::new_connected(chan_id, sock);

        // Initial subscribe: READ.
        if let Err(e) = bridge.reactor_mut().register_udp(chan_id, udp.socket_mut(), Interest::READ) {
            let _ = tx.send(Err(e));
            return;
        }

        if bridge.install_channel(chan_id, crate::MioIo::Udp(udp)).is_err() {
            metrics.accept_rejected_total.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(Err(std::io::Error::other("dataplane capacity exhausted")));
            return;
        }
        metrics.accepted_total.fetch_add(1, Ordering::Relaxed);

        let _ = tx.send(Ok(()));

        // Phase A: running.
        while !draining.load(Ordering::Acquire) {
            let _ = bridge.tick(cfg.budget);
            let _ = crate::apply_pending_udp(&mut bridge);
            if cfg.budget.max_nanos == 0 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        // Phase B: graceful draining.
        let drain_timeout = cfg.drain_timeout;
        let deadline = Instant::now() + drain_timeout;
        bridge.__with_reactor_and_channels(|_r, chans| {
            for slot in chans.iter_mut() {
                if let Some(ch) = slot.as_mut() {
                    ch.enter_draining(true, drain_timeout, 0);
                }
            }
        });
        while metrics.active_connections.load(Ordering::Relaxed) > 0 && Instant::now() < deadline {
            let _ = bridge.tick(cfg.budget);
            let _ = crate::apply_pending_udp(&mut bridge);
            if cfg.budget.max_nanos == 0 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        // bridge dropped here.
    });

    match rx.recv() {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_disconnected) => {
            let _ = handle.join();
            Err(std::io::Error::other("dataplane thread startup failed"))
        }
    }
}

/// Spawn a connected-UDP dataplane (mio backend).
///
/// This is a thin wrapper around [`try_spawn_udp_dataplane`].
///
/// Production note: prefer the fallible API to avoid panics in library code.
pub fn spawn_udp_dataplane<A>(
    cfg: DataPlaneConfig,
    remote: SocketAddr,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<thread::JoinHandle<()>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    try_spawn_udp_dataplane(cfg, remote, draining, app, metrics)
}
