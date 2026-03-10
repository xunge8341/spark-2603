//! TCP dataplane (mio backend).

use crate::{acceptor, MioBridge, MioReactor, QueueExecutor};

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::async_bridge::FrameDecoderProfile;
use spark_transport::KernelError;
use spark_transport::{AsyncBridge, DataPlaneConfig, DataPlaneMetrics};

use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// A spawned TCP dataplane instance.
///
/// Why we return `local_addr`:
/// - Tests and embedding scenarios frequently want `bind=127.0.0.1:0` (ephemeral port)
///   while still being able to connect to the actual bound port.
/// - This is a transport-core requirement for reliable dogfooding and avoids global port
///   allocation races in CI.
#[derive(Debug)]
pub struct TcpDataplaneHandle {
    pub join: thread::JoinHandle<()>,
    pub local_addr: SocketAddr,
}

/// Try-spawn a Windows-friendly TCP dataplane (mio backend).
///
/// Rust best-practice:
/// - fallible *socket* setup (bind / nonblocking) happens in the caller thread;
/// - the dataplane driver itself may contain intentionally `!Send` types (single-threaded event-loop semantics),
///   so it is constructed inside the spawned thread.
///
/// A small startup handshake is used to keep this API fallible.
pub fn try_spawn_tcp_dataplane<A>(
    cfg: DataPlaneConfig,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    let cfg = cfg.normalized();
    if let Err(e) = cfg.validate() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
    }
    let listener = TcpListener::bind(cfg.bind)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;

    // Construct !Send driver inside the spawned thread.
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

        let _ = tx.send(Ok(()));

        while !draining.load(Ordering::Acquire) {
            // Accept phase.
            if let Err(_e) = acceptor::accept_tick(&listener, &mut bridge, &cfg, &metrics) {
                break;
            }

            // Drive one tick.
            let _ = bridge.tick(cfg.budget);

            // Apply deferred interest changes.
            let _ = crate::apply_pending_tcp(&mut bridge);

            if cfg.budget.max_nanos == 0 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        // Phase B: graceful draining (Kestrel/Netty style).
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
            let _ = crate::apply_pending_tcp(&mut bridge);
            if cfg.budget.max_nanos == 0 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        // bridge dropped here.
    });

    match rx.recv() {
        Ok(Ok(())) => Ok(TcpDataplaneHandle {
            join: handle,
            local_addr,
        }),
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

/// Try-spawn a TCP dataplane with an explicit framing profile.
///
/// This is primarily used for control-plane dogfooding (e.g. HTTP/1 management server on spark-transport).
pub fn try_spawn_tcp_dataplane_with_framing<A>(
    cfg: DataPlaneConfig,
    framing: FrameDecoderProfile,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    // Options-style wrapper: embed framing into config and reuse the main spawn.
    let cfg = cfg.with_framing(framing);
    try_spawn_tcp_dataplane(cfg, draining, app, metrics)
}

/// Spawn a Windows-friendly TCP dataplane (mio backend).
///
/// This is a thin wrapper around [`try_spawn_tcp_dataplane`].
///
/// Production note: prefer the fallible API to avoid panics in library code.
pub fn spawn_tcp_dataplane<A>(
    cfg: DataPlaneConfig,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    try_spawn_tcp_dataplane(cfg, draining, app, metrics)
}
