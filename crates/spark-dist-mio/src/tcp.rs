//! Default TCP distribution entry point (mio backend).

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::HostSpec;
use spark_transport::KernelError;

/// Run a TCP dataplane (mio backend) + transport-backed mgmt-plane (ember) in blocking mode.
///
/// - mgmt-plane: `spark-ember` (dogfooding on `spark-transport` when enabled)
/// - dataplane: `spark-transport-mio` (Netty-like channel semantics)
pub fn run_tcp_default<S>(spec: HostSpec<S>) -> std::io::Result<()>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    spark_ember::run_blocking(
        spec,
        |spec, draining| {
            spark_transport_mio::spawn_tcp_dataplane(
                spec.dataplane.clone(),
                draining,
                std::sync::Arc::clone(&spec.service),
                std::sync::Arc::clone(&spec.metrics),
            )
            .map(|h| h.join)
        },
        |cfg, draining, service, metrics| {
            // DECISION: mgmt-plane must ride the same transport backend as the distribution.
            // This closure is injected into `spark-ember` to keep ember runtime-neutral.
            spark_transport_mio::try_spawn_tcp_dataplane(cfg, draining, service, metrics).map(|h| {
                spark_ember::http1::SpawnedTcpDataplane {
                    join: h.join,
                    local_addr: h.local_addr,
                }
            })
        },
    )
}
