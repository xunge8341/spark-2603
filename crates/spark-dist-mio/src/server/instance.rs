use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::HostSpec;
use spark_transport::KernelError;

use std::net::SocketAddr;

/// Transport selection for the distribution server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    UdpConnected { remote: SocketAddr },
}

/// A built server instance.
///
/// This owns the runtime-neutral `HostSpec` and chooses a dataplane transport.
pub struct Server<S> {
    pub(crate) spec: HostSpec<S>,
    pub(crate) transport: Transport,
}

impl<S> Server<S>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    /// Run the server in blocking mode.
    ///
    /// - mgmt-plane: `spark-ember` (std-only)
    /// - dataplane: mio backend (thread-per-dataplane)
    pub fn run(self) -> std::io::Result<()> {
        match self.transport {
            Transport::Tcp => crate::run_tcp_default(self.spec),
            Transport::UdpConnected { remote } => crate::run_udp_connected_default(self.spec, remote),
        }
    }

    #[inline]
    pub fn host_spec(&self) -> &HostSpec<S> {
        &self.spec
    }
}
