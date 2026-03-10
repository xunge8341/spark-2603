use mio::{Events, Poll, Token};
use spark_transport::reactor::{Interest, KernelEvent, Reactor};
use spark_transport::{Budget, KernelError, Result};

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Tcp,
    Udp,
}

#[derive(Debug)]
pub struct MioReactor {
    poll: Poll,
    events: Events,
    kind: HashMap<u32, SourceKind>,
    pending_tcp: HashMap<u32, Interest>,
    current_tcp: HashMap<u32, Interest>,
    pending_udp: HashMap<u32, Interest>,
    current_udp: HashMap<u32, Interest>,
}

/// Lookup interface for mutable access to TCP streams by channel id.
///
/// This avoids returning `&mut` from a closure (which would require explicit lifetimes).
pub trait TcpStreamLookup {
    fn get_stream_mut(&mut self, chan_id: u32) -> Option<&mut mio::net::TcpStream>;
}

/// Lookup interface for mutable access to UDP sockets by channel id.
pub trait UdpSocketLookup {
    fn get_socket_mut(&mut self, chan_id: u32) -> Option<&mut mio::net::UdpSocket>;
}

impl MioReactor {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            poll: Poll::new()?,
            events: Events::with_capacity(1024),
            kind: HashMap::new(),
            pending_tcp: HashMap::new(),
            current_tcp: HashMap::new(),
            pending_udp: HashMap::new(),
            current_udp: HashMap::new(),
        })
    }

    #[inline]
    pub fn registry(&self) -> &mio::Registry {
        self.poll.registry()
    }
    /// Best-effort removal of reactor bookkeeping for a channel.
    ///
    /// This is used when the transport layer has already dropped the IO object
    /// (so we cannot call mio::Registry::deregister), but we still must prevent
    /// unbounded growth of internal maps.
    #[inline]
    pub fn forget(&mut self, chan_id: u32) {
        self.pending_tcp.remove(&chan_id);
        self.current_tcp.remove(&chan_id);
        self.pending_udp.remove(&chan_id);
        self.current_udp.remove(&chan_id);
        self.kind.remove(&chan_id);
    }


    pub fn register_tcp(
        &mut self,
        chan_id: u32,
        stream: &mut mio::net::TcpStream,
        interest: Interest,
    ) -> std::io::Result<()> {
        self.poll
            .registry()
            .register(stream, Token(chan_id as usize), to_mio_interest(interest))?;
        self.kind.insert(chan_id, SourceKind::Tcp);
        self.current_tcp.insert(chan_id, interest);
        Ok(())
    }

    pub fn register_udp(
        &mut self,
        chan_id: u32,
        sock: &mut mio::net::UdpSocket,
        interest: Interest,
    ) -> std::io::Result<()> {
        self.poll
            .registry()
            .register(sock, Token(chan_id as usize), to_mio_interest(interest))?;
        self.kind.insert(chan_id, SourceKind::Udp);
        self.current_udp.insert(chan_id, interest);
        Ok(())
    }

    pub fn apply_pending_tcp<L>(&mut self, lookup: &mut L) -> std::io::Result<()>
    where
        L: TcpStreamLookup,
    {
        if self.pending_tcp.is_empty() {
            return Ok(());
        }

        let drained: Vec<(u32, Interest)> = self.pending_tcp.drain().collect();
        for (chan_id, interest) in drained {
            if self.current_tcp.get(&chan_id).copied() == Some(interest) {
                continue;
            }

            if let Some(stream) = lookup.get_stream_mut(chan_id) {
                if interest.is_empty() {
                    // Contract: Interest::empty means unsubscribe (PauseRead and no write backlog).
                    // mio requires explicit deregister.
                    let _ = self.poll.registry().deregister(stream);
                    self.current_tcp.remove(&chan_id);
                    // Keep kind until the IO is actually dropped; we may need to re-register later.
                } else {
                    // IMPORTANT: after a deregister, mio requires register (not reregister).
                    if self.current_tcp.contains_key(&chan_id) {
                        self.poll.registry().reregister(
                            stream,
                            Token(chan_id as usize),
                            to_mio_interest(interest),
                        )?;
                    } else {
                        self.poll.registry().register(
                            stream,
                            Token(chan_id as usize),
                            to_mio_interest(interest),
                        )?;
                    }
                    self.current_tcp.insert(chan_id, interest);
                    self.kind.insert(chan_id, SourceKind::Tcp);
                }
            } else {
                // Transport already dropped the stream: prevent map growth.
                self.current_tcp.remove(&chan_id);
                if !self.current_udp.contains_key(&chan_id) {
                    self.kind.remove(&chan_id);
                }
            }
        }
        Ok(())
    }

    pub fn apply_pending_udp<L>(&mut self, lookup: &mut L) -> std::io::Result<()>
    where
        L: UdpSocketLookup,
    {
        if self.pending_udp.is_empty() {
            return Ok(());
        }

        let drained: Vec<(u32, Interest)> = self.pending_udp.drain().collect();
        for (chan_id, interest) in drained {
            if self.current_udp.get(&chan_id).copied() == Some(interest) {
                continue;
            }

            if let Some(sock) = lookup.get_socket_mut(chan_id) {
                if interest.is_empty() {
                    let _ = self.poll.registry().deregister(sock);
                    self.current_udp.remove(&chan_id);
                    // Keep kind until the IO is actually dropped; we may need to re-register later.
                } else {
                    if self.current_udp.contains_key(&chan_id) {
                        self.poll.registry().reregister(
                            sock,
                            Token(chan_id as usize),
                            to_mio_interest(interest),
                        )?;
                    } else {
                        self.poll.registry().register(
                            sock,
                            Token(chan_id as usize),
                            to_mio_interest(interest),
                        )?;
                    }
                    self.current_udp.insert(chan_id, interest);
                    self.kind.insert(chan_id, SourceKind::Udp);
                }
            } else {
                self.current_udp.remove(&chan_id);
                if !self.current_tcp.contains_key(&chan_id) {
                    self.kind.remove(&chan_id);
                }
            }
        }
        Ok(())
    }
}

impl Reactor for MioReactor {
    fn poll_into(
        &mut self,
        budget: Budget,
        out: &mut [MaybeUninit<KernelEvent>],
    ) -> Result<usize> {
        let timeout = if budget.max_nanos == 0 {
            Some(Duration::from_millis(0))
        } else {
            Some(Duration::from_nanos(budget.max_nanos))
        };

        self.poll
            .poll(&mut self.events, timeout)
            .map_err(|_| KernelError::Internal(spark_transport::error_codes::ERR_MIO_POLL_FAILED))?;

        let mut n = 0usize;
        let limit = (budget.max_events.max(1) as usize).min(out.len());
        for ev in self.events.iter() {
            if n >= limit {
                break;
            }
            let chan_id = ev.token().0 as u32;

            // IMPORTANT cross-platform semantics:
            // - `read_closed` indicates the peer half-closed its write side (FIN). This MUST NOT be
            //   surfaced as a full close event because the transport layer still needs to read any
            //   remaining bytes and may still write responses.
            // - `write_closed` indicates the peer is no longer reading; treat it as terminal.
            if ev.is_error() || ev.is_write_closed() {
                out[n].write(KernelEvent::Closed { chan_id });
                n += 1;
                continue;
            }

            // Treat read-closed as readable to let the transport observe EOF via a zero-length read.
            if ev.is_readable() || ev.is_read_closed() {
                out[n].write(KernelEvent::Readable { chan_id });
                n += 1;
                if n >= out.len() {
                    break;
                }
            }

            if ev.is_writable() {
                out[n].write(KernelEvent::Writable { chan_id });
                n += 1;
            }
        }

        Ok(n)
    }

    fn register(
        &mut self,
        chan_id: u32,
        interest: Interest,
    ) -> core::result::Result<(), KernelError> {
        // 说明：mio 没有 NONE；我们把 empty 作为“取消订阅”语义，延迟到 apply_pending_* 时执行 deregister。
        // 需要根据 chan_id 对应的 IO 类型路由到 tcp/udp pending 队列。
        match self.kind.get(&chan_id).copied() {
            Some(SourceKind::Udp) => {
                self.pending_udp.insert(chan_id, interest);
            }
            Some(SourceKind::Tcp) | None => {
                self.pending_tcp.insert(chan_id, interest);
            }
        }
        Ok(())
    }
}

#[inline]
fn to_mio_interest(i: Interest) -> mio::Interest {
    match (i.contains(Interest::READ), i.contains(Interest::WRITE)) {
        (true, true) => mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        (true, false) => mio::Interest::READABLE,
        (false, true) => mio::Interest::WRITABLE,
        (false, false) => mio::Interest::READABLE,
    }
}