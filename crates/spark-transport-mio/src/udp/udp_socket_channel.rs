use mio::net::UdpSocket;

use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadData, ReadOutcome};
use spark_transport::{KernelError, Result};

/// OS UDP socket channel (connected).
///
/// 说明：
/// - IoOps 接口不携带地址参数，因此这里采用“connected UDP”语义；
/// - 对上层暴露的仍是统一的 WouldBlock/Closed/Timeout 等错误分类。
#[derive(Debug)]
pub struct UdpSocketChannel {
    id: u32,
    sock: UdpSocket,
    closed: bool,

    /// 累计截断次数（best-effort）。
    rx_truncated_total: u64,
}

impl UdpSocketChannel {
    pub fn new_connected(id: u32, sock: UdpSocket) -> Self {
        Self { id, sock, closed: false, rx_truncated_total: 0 }
    }

    #[inline]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[inline]
    pub fn socket_mut(&mut self) -> &mut UdpSocket {
        &mut self.sock
    }

    #[inline]
    pub fn rx_truncated_total(&self) -> u64 {
        self.rx_truncated_total
    }
}


/// Windows 上 UDP 收包在“缓冲区不足”时会返回 WSAEMSGSIZE（10040），
/// 表示消息被截断（但底层通常已经把前 dst.len() 字节写入到缓冲区）。
///
/// 由于 Rust/Mio 的 `recv` API 在错误时无法返回“实际写入字节数”，这里采用 best-effort：
/// - 返回 Ok(ReadOutcome) 且 n=dst.len()（表示缓冲区被填满）
/// - truncated=true，并累加计数器
///
/// 这能让上层观测到“发生过截断”，并据此调整接收缓冲区/协议策略。
#[inline]
fn is_udp_truncation_error(e: &std::io::Error) -> bool {
    // WinSock: WSAEMSGSIZE = 10040
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(10040)
    }
    #[cfg(not(windows))]
    {
        let _ = e;
        false
    }
}

impl IoOps for UdpSocketChannel {
    fn capabilities(&self) -> ChannelCaps {
        caps::DATAGRAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }

        match self.sock.recv(dst) {
            Ok(n) => {
                // UDP: Ok(0) 也是合法 datagram（空包），不应被视为 WouldBlock。
                // 由于 IoOps 缺少 recvmsg(MSG_TRUNC) 等精确信息，这里采用 best-effort：
                // 若填满 dst，则标记 truncated=true 并累加计数。
                let truncated = !dst.is_empty() && n == dst.len();
                if truncated {
                    self.rx_truncated_total = self.rx_truncated_total.saturating_add(1);
                }
                Ok(ReadOutcome {
                    n,
                    boundary: MsgBoundary::Complete,
                    truncated,
                    data: ReadData::Copied,
                })
            }
            Err(e) => {
                // Windows 上“缓冲区不足”会走错误分支，但语义上是“收到并截断”。
                if is_udp_truncation_error(&e) {
                    if !dst.is_empty() {
                        self.rx_truncated_total = self.rx_truncated_total.saturating_add(1);
                    }
                    return Ok(ReadOutcome {
                        n: dst.len(),
                        boundary: MsgBoundary::Complete,
                        truncated: !dst.is_empty(),
                        data: ReadData::Copied,
                    });
                }
                Err(spark_transport::io::classify_connected_udp_recv_error(&e))
            }
        }
    }

    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        match self.sock.send(data) {
            Ok(n) => Ok(n),
            Err(e) => Err(spark_transport::io::classify_io_error(&e)),
        }
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        // UDP: best-effort 关闭语义。
        self.closed = true;
        Ok(())
    }
}


impl spark_transport::async_bridge::dyn_channel::DynChannel for UdpSocketChannel {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
