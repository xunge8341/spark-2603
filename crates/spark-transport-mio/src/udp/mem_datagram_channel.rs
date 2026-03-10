use std::collections::VecDeque;

use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadData, ReadOutcome};
use spark_transport::{KernelError, Result};

/// 简单的内存 Datagram 通道（用于 bring-up 与 contract tests）。
///
/// - RX：`push_rx()` 入队一个 datagram；
/// - TX：`try_write()` 将 payload 追加到 `tx_log`。
#[derive(Debug, Default)]
pub struct MemDatagramChannel {
    rx_q: VecDeque<Vec<u8>>,
    pub tx_log: Vec<Vec<u8>>,
    closed: bool,
}

impl MemDatagramChannel {
    pub fn new() -> Self {
        Self { rx_q: VecDeque::new(), tx_log: Vec::new(), closed: false }
    }

    pub fn push_rx(&mut self, datagram: Vec<u8>) {
        if self.closed {
            return;
        }
        self.rx_q.push_back(datagram);
    }
}

impl IoOps for MemDatagramChannel {
    fn capabilities(&self) -> ChannelCaps {
        // 目前为 copy-based RX；后续可在不破坏语义的前提下演进到 Lease/arena。
        caps::DATAGRAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        let Some(pkt) = self.rx_q.pop_front() else {
            return Err(KernelError::WouldBlock);
        };

        let n = pkt.len().min(dst.len());
        dst[..n].copy_from_slice(&pkt[..n]);

        Ok(ReadOutcome {
            n,
            boundary: MsgBoundary::Complete,
            truncated: pkt.len() > dst.len(),
            data: ReadData::Copied,
        })
    }

    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        self.tx_log.push(data.to_vec());
        Ok(data.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        // 内存通道：close 幂等。
        self.closed = true;
        self.rx_q.clear();
        Ok(())
    }
}


impl spark_transport::async_bridge::dyn_channel::DynChannel for MemDatagramChannel {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
