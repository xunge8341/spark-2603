//! mio backend IO enum (leaf-only).
//!
//! Goal:
//! - Remove `Any` downcast from the hot-ish "apply pending interest" path.
//! - Keep the transport core runtime-neutral (no mio types leak upward).
//! - Stay developer-friendly: `enum` match is readable, avoids generic explosion.

use mio::net::{TcpStream, UdpSocket};

use crate::tcp::TcpChannel;
use crate::udp::{MemDatagramChannel, UdpSocketChannel};

use spark_transport::io::{ChannelCaps, IoOps, ReadOutcome, Result, RxToken};

/// Leaf IO sum type for mio backend.
///
/// Design notes:
/// - `spark-transport` core works with any `IoOps` implementation.
/// - In mio backend we use an enum to keep dispatch static and explicit.
#[derive(Debug)]
pub enum MioIo {
    Tcp(TcpChannel),
    Udp(UdpSocketChannel),
    MemDatagram(MemDatagramChannel),
}

impl MioIo {
    #[inline]
    pub fn tcp_stream_mut(&mut self) -> Option<&mut TcpStream> {
        match self {
            MioIo::Tcp(ch) => Some(ch.stream_mut()),
            _ => None,
        }
    }

    #[inline]
    pub fn udp_socket_mut(&mut self) -> Option<&mut UdpSocket> {
        match self {
            MioIo::Udp(ch) => Some(ch.socket_mut()),
            _ => None,
        }
    }
}

impl IoOps for MioIo {
    #[inline]
    fn capabilities(&self) -> ChannelCaps {
        match self {
            MioIo::Tcp(ch) => ch.capabilities(),
            MioIo::Udp(ch) => ch.capabilities(),
            MioIo::MemDatagram(ch) => ch.capabilities(),
        }
    }

    #[inline]
    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        match self {
            MioIo::Tcp(ch) => ch.try_read_lease(),
            MioIo::Udp(ch) => ch.try_read_lease(),
            MioIo::MemDatagram(ch) => ch.try_read_lease(),
        }
    }

    #[inline]
    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        match self {
            MioIo::Tcp(ch) => ch.try_read_into(dst),
            MioIo::Udp(ch) => ch.try_read_into(dst),
            MioIo::MemDatagram(ch) => ch.try_read_into(dst),
        }
    }

    #[inline]
    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        match self {
            MioIo::Tcp(ch) => ch.try_write(data),
            MioIo::Udp(ch) => ch.try_write(data),
            MioIo::MemDatagram(ch) => ch.try_write(data),
        }
    }

    #[inline]
    fn try_write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize> {
        match self {
            MioIo::Tcp(ch) => ch.try_write_vectored(bufs),
            MioIo::Udp(ch) => ch.try_write_vectored(bufs),
            MioIo::MemDatagram(ch) => ch.try_write_vectored(bufs),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        match self {
            MioIo::Tcp(ch) => ch.flush(),
            MioIo::Udp(ch) => ch.flush(),
            MioIo::MemDatagram(ch) => ch.flush(),
        }
    }

    #[inline]
    fn rx_ptr_len(&mut self, tok: RxToken) -> Option<(*const u8, usize)> {
        match self {
            MioIo::Tcp(ch) => ch.rx_ptr_len(tok),
            MioIo::Udp(ch) => ch.rx_ptr_len(tok),
            MioIo::MemDatagram(ch) => ch.rx_ptr_len(tok),
        }
    }

    #[inline]
    fn release_rx(&mut self, tok: RxToken) {
        match self {
            MioIo::Tcp(ch) => ch.release_rx(tok),
            MioIo::Udp(ch) => ch.release_rx(tok),
            MioIo::MemDatagram(ch) => ch.release_rx(tok),
        }
    }

    #[inline]
    fn close(&mut self) -> Result<()> {
        match self {
            MioIo::Tcp(ch) => ch.close(),
            MioIo::Udp(ch) => ch.close(),
            MioIo::MemDatagram(ch) => ch.close(),
        }
    }
}


impl spark_transport::async_bridge::dyn_channel::DynChannel for MioIo {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
