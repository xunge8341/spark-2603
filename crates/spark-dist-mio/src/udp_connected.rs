//! Default connected-UDP distribution entry point (mio backend).

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::HostSpec;
use spark_transport::KernelError;

use std::net::SocketAddr;

/// Run a connected-UDP dataplane (mio backend) + transport-backed mgmt-plane (ember) in blocking mode.
///
/// Notes:
/// - UDP is modeled as a single connected socket channel in the current contract.
pub fn run_udp_connected_default<S>(spec: HostSpec<S>, remote: SocketAddr) -> std::io::Result<()>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    spark_ember::run_blocking(
        spec,
        |spec, draining| {
            spark_transport_mio::spawn_udp_dataplane(
                spec.dataplane.clone(),
                remote,
                draining,
                std::sync::Arc::clone(&spec.service),
                std::sync::Arc::clone(&spec.metrics),
            )
        },
        |cfg, draining, service, metrics| {
            spark_transport_mio::try_spawn_tcp_dataplane(cfg, draining, service, metrics).map(|h| {
                spark_ember::http1::SpawnedTcpDataplane {
                    join: h.join,
                    local_addr: h.local_addr,
                }
            })
        },
    )
}
