//! Connected-UDP profile entry points (mio backend).

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::KernelError;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics};

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

/// Fallible connected-UDP dataplane spawn.
pub fn try_spawn<A>(
    cfg: DataPlaneConfig,
    remote: SocketAddr,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<thread::JoinHandle<()>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    crate::try_spawn_udp_dataplane(cfg, remote, draining, app, metrics)
}

/// Convenience connected-UDP dataplane spawn.
pub fn spawn<A>(
    cfg: DataPlaneConfig,
    remote: SocketAddr,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<thread::JoinHandle<()>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    crate::spawn_udp_dataplane(cfg, remote, draining, app, metrics)
}

/// Apply deferred UDP interest changes.
pub fn apply_pending<E, A, Ev>(
    bridge: &mut spark_transport::async_bridge::ChannelDriver<crate::MioReactor, E, A, Ev, crate::MioIo>,
) -> std::io::Result<()>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: spark_transport::evidence::EvidenceHandle,
{
    crate::apply_pending_udp(bridge)
}
