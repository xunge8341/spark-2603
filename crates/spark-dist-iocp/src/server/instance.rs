use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::HostSpec;
use spark_transport::KernelError;

/// Transport selection for the distribution server.
///
/// DECISION: IOCP distribution starts with TCP (P0 for internal self-bootstrapping).
/// UDP and other transports will be added once IOCP semantics are proven by contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
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
    pub fn run(self) -> std::io::Result<()> {
        match self.transport {
            Transport::Tcp => crate::run_tcp_default(self.spec),
        }
    }

    #[inline]
    pub fn host_spec(&self) -> &HostSpec<S> {
        &self.spec
    }
}
