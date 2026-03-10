use mio::net::TcpStream;
use std::io::{IoSlice, Read, Write};
use std::net::Shutdown;

use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadData, ReadOutcome};
use spark_transport::{KernelError, Result, RxToken};

use super::rx_token;

/// TCP IO backend（mio）。
///
/// 这是**低层 IO 操作**（接近 Netty 的 `Channel.Unsafe`）：
/// - 只关心非阻塞 read/write/flush/close；
/// - 不负责粘包拆包、背压水位线、pipeline 事件传播等语义。
///
/// 上层语义在 `spark-transport` 的状态机中实现，从而实现：
/// - 核心运行时中立；
/// - 后端可替换（mio/uring/serial/…）。
#[derive(Debug)]
pub struct TcpChannel {
    id: u32,
    stream: TcpStream,

    rx_buf: Box<[u8]>,
    rx_len: usize,
    rx_gen: u32,
    leased: bool,

    /// 本端是否已调用 close（用于幂等与错误语义收敛）。
    closed: bool,
}

impl TcpChannel {

#[cfg(windows)]
fn drain_read_best_effort(&mut self) {
    // Best-effort: drain any pending inbound bytes so that subsequent close() does not reset the peer.
    // This is especially important on Windows where closing with unread data may result in an abortive close.
    let mut buf = [0u8; 4096];
    for _ in 0..16 {
        match self.stream.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

    pub fn new(id: u32, stream: TcpStream, max_frame: usize) -> std::io::Result<Self> {
        let _ = stream.set_nodelay(true);
        Ok(Self {
            id,
            stream,
            rx_buf: vec![0u8; max_frame.max(1)].into_boxed_slice(),
            rx_len: 0,
            rx_gen: 0,
            leased: false,
            closed: false,
        })
    }

    #[inline]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[inline]
    pub fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    /// View leased buffer as pointer/len.
    ///
    /// 注意：这是 bring-up 期的“短生命周期零拷贝视图”。
    /// 上层应尽快 materialize（拷贝）或升级为 ref-count/arena 方案。
    pub fn rx_ptr_len(&self, tok: RxToken) -> Option<(*const u8, usize)> {
        let (id, gen) = rx_token::decode(tok);
        if id != self.id || !self.leased || gen != self.rx_gen {
            return None;
        }
        Some((self.rx_buf.as_ptr(), self.rx_len))
    }

    pub fn release_rx(&mut self, tok: RxToken) {
        let (id, gen) = rx_token::decode(tok);
        if id == self.id && self.leased && gen == self.rx_gen {
            self.leased = false;
            self.rx_len = 0;
        }
    }
}


impl IoOps for TcpChannel {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM | caps::RELIABLE | caps::ORDERED | caps::ZERO_COPY_RX
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        if self.leased {
            return Err(KernelError::WouldBlock);
        }

        match self.stream.read(&mut self.rx_buf[..]) {
            Ok(0) => Err(KernelError::Eof),
            Ok(n) => {
                self.rx_len = n;
                self.rx_gen = self.rx_gen.wrapping_add(1);
                self.leased = true;
                let tok = rx_token::encode(self.id, self.rx_gen);
                Ok(ReadOutcome {
                    n,
                    boundary: MsgBoundary::None,
                    truncated: false,
                    data: ReadData::Token(tok),
                })
            }
            Err(e) => Err(spark_transport::io::classify_io_error(&e)),
        }
    }

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }

        match self.stream.read(dst) {
            Ok(0) => Err(KernelError::Eof),
            Ok(n) => Ok(ReadOutcome {
                n,
                boundary: MsgBoundary::None,
                truncated: false,
                data: ReadData::Copied,
            }),
            Err(e) => Err(spark_transport::io::classify_io_error(&e)),
        }
    }

    fn try_write(&mut self, src: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }

        match self.stream.write(src) {
            Ok(n) if n > 0 => Ok(n),
            Ok(_) => Err(KernelError::WouldBlock),
            Err(e) => Err(spark_transport::io::classify_io_error(&e)),
        }
    }

    fn try_write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }

        // Convert to std IoSlice; skip empties.
        let mut tmp: [IoSlice<'_>; 8] = [IoSlice::new(&[]); 8];
        let mut n = 0usize;
        for b in bufs.iter().take(tmp.len()) {
            if b.is_empty() {
                continue;
            }
            tmp[n] = IoSlice::new(b);
            n += 1;
        }
        if n == 0 {
            return Ok(0);
        }

        match self.stream.write_vectored(&tmp[..n]) {
            Ok(w) if w > 0 => Ok(w),
            Ok(_) => Err(KernelError::WouldBlock),
            Err(e) => Err(spark_transport::io::classify_io_error(&e)),
        }
    }

    fn flush(&mut self) -> Result<()> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        self.stream
            .flush()
            .map_err(|e| spark_transport::io::classify_io_error(&e))
    }

    #[inline]
    fn rx_ptr_len(&mut self, tok: RxToken) -> Option<(*const u8, usize)> {
        TcpChannel::rx_ptr_len(self, tok)
    }

    #[inline]
    fn release_rx(&mut self, tok: RxToken) {
        TcpChannel::release_rx(self, tok)
    }

    fn close(&mut self) -> Result<()> {
        // Netty/DotNetty 语义：close 必须幂等。
        if self.closed {
            return Ok(());
        }
        self.closed = true;

        // Best-effort: drain any pending inbound bytes before teardown.
        // On Windows, closing a socket with unread data can cause an abortive close (RST/10054) to the peer.
        #[cfg(windows)]
        self.drain_read_best_effort();

        // IMPORTANT (cross-platform contract):
        // - peer may have half-closed its write side (FIN observed as read EOF)
        // - we still must be able to flush pending outbound before teardown
        // - on Windows, an aggressive `shutdown(Both)` can lead to abortive closes (RST)
        //   in some timing windows.
        //
        // Best-effort: shut down only the WRITE side to send a FIN once the OS send buffer drains.
        // Dropping the stream will complete the close.
        let _ = self.stream.shutdown(Shutdown::Write);
        Ok(())
    }
}



impl Drop for TcpChannel {
    fn drop(&mut self) {
        // Defensive: ensure we attempt a graceful shutdown even if callers forgot to call close().
        // Also drain any pending inbound bytes on Windows to avoid an abortive close (RST).
        #[cfg(windows)]
        self.drain_read_best_effort();
        let _ = self.stream.shutdown(Shutdown::Write);
    }
}

impl spark_transport::async_bridge::dyn_channel::DynChannel for TcpChannel {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
