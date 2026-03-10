//! Default TCP distribution entry point (IOCP path).

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::HostSpec;
use spark_transport::KernelError;

/// Run a TCP dataplane (IOCP path) + transport-backed mgmt-plane (ember) in blocking mode.
///
/// DECISION: use the same hosting topology as `spark-dist-mio`.
/// This keeps "ASP.NET Core style" assembly stable while we evolve the backend.
pub fn run_tcp_default<S>(spec: HostSpec<S>) -> std::io::Result<()>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    spark_ember::run_blocking(
        spec,
        |spec, draining| {
            spark_transport_iocp::spawn_tcp_dataplane(
                spec.dataplane.clone(),
                draining,
                std::sync::Arc::clone(&spec.service),
                std::sync::Arc::clone(&spec.metrics),
            )
            .map(|h| h.join)
        },
        |cfg, draining, service, metrics| {
            // DECISION: route mgmt-plane through the same IOCP boundary as the distribution.
            spark_transport_iocp::try_spawn_tcp_dataplane(cfg, draining, service, metrics).map(|h| {
                spark_ember::http1::SpawnedTcpDataplane {
                    join: h.join,
                    local_addr: h.local_addr,
                }
            })
        },
    )
}
