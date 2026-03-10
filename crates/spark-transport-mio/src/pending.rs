//! Apply deferred mio interest changes.
//!
//! `spark-transport` is runtime-neutral and does not know about mio socket types.
//! This module provides the leaf-only glue that maps `chan_id -> mio socket`.

use crate::{MioIo, MioReactor, TcpStreamLookup, UdpSocketLookup};

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::KernelError;

/// Apply deferred TCP interest changes recorded by the mio reactor.
pub fn apply_pending_tcp<E, A, Ev>(
    bridge: &mut spark_transport::async_bridge::ChannelDriver<MioReactor, E, A, Ev, MioIo>,
) -> std::io::Result<()>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: spark_transport::evidence::EvidenceHandle,
{
    use spark_transport::async_bridge::channel::Channel;

    struct Lookup<'a, A, Ev>
    where
        A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
        Ev: spark_transport::evidence::EvidenceHandle,
    {
        channels: &'a mut Vec<Option<Channel<A, Ev, MioIo>>>,
    }

    impl<'a, A, Ev> TcpStreamLookup for Lookup<'a, A, Ev>
    where
        A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
        Ev: spark_transport::evidence::EvidenceHandle,
    {
        fn get_stream_mut(&mut self, chan_id: u32) -> Option<&mut mio::net::TcpStream> {
            let idx = spark_transport::async_bridge::chan_index(chan_id);
            let Some(Some(ch)) = self.channels.get_mut(idx) else {
                return None;
            };

            // Leaf-only: explicit enum dispatch (no downcast).
            ch.io_mut().tcp_stream_mut()
        }
    }

    bridge.__with_reactor_and_channels(|reactor, channels| {
        let mut lookup: Lookup<'_, A, Ev> = Lookup { channels };
        reactor.apply_pending_tcp(&mut lookup)
    })
}

/// Apply deferred UDP interest changes recorded by the mio reactor.
pub fn apply_pending_udp<E, A, Ev>(
    bridge: &mut spark_transport::async_bridge::ChannelDriver<MioReactor, E, A, Ev, MioIo>,
) -> std::io::Result<()>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: spark_transport::evidence::EvidenceHandle,
{
    use spark_transport::async_bridge::channel::Channel;

    struct Lookup<'a, A, Ev>
    where
        A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
        Ev: spark_transport::evidence::EvidenceHandle,
    {
        channels: &'a mut Vec<Option<Channel<A, Ev, MioIo>>>,
    }

    impl<'a, A, Ev> UdpSocketLookup for Lookup<'a, A, Ev>
    where
        A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
        Ev: spark_transport::evidence::EvidenceHandle,
    {
        fn get_socket_mut(&mut self, chan_id: u32) -> Option<&mut mio::net::UdpSocket> {
            let idx = spark_transport::async_bridge::chan_index(chan_id);
            let Some(Some(ch)) = self.channels.get_mut(idx) else {
                return None;
            };
            ch.io_mut().udp_socket_mut()
        }
    }

    bridge.__with_reactor_and_channels(|reactor, channels| {
        let mut lookup: Lookup<'_, A, Ev> = Lookup { channels };
        reactor.apply_pending_udp(&mut lookup)
    })
}
