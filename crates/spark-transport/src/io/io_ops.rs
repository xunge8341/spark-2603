use super::{ChannelCaps, KernelError, ReadOutcome, Result, RxToken};

/// Low-level IO operations for a concrete backend.
///
/// This trait is intentionally minimal and maps closely to non-blocking syscalls.
///
/// In Netty terms, this is closest to `Channel.Unsafe` (internal), not the public
/// `Channel` semantic object.
pub trait IoOps {
    fn capabilities(&self) -> ChannelCaps;

    /// Lease-based read (optional).
    ///
    /// Contract:
    /// - Backends that support leasing must return `Ok(ReadOutcome{ data: Token(..) })`.
    /// - Backends that do not support leasing must return `Err(KernelError::Unsupported)`
    ///   and implement `try_read_into`.
    /// - Returning `Ok(..Copied)` from this method is a contract violation.
    fn try_read_lease(&mut self) -> Result<ReadOutcome>;

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome>;

    fn try_write(&mut self, data: &[u8]) -> Result<usize>;

    /// Best-effort vectored write.
    ///
    /// Design notes:
    /// - This keeps the core trait `no_std` friendly by avoiding `std::io::IoSlice`.
    /// - Backends may override with a true `writev`/gather syscall.
    /// - The default implementation falls back to repeated `try_write` calls.
    #[inline]
    fn try_write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize> {
        let mut written = 0usize;
        for b in bufs {
            if b.is_empty() {
                continue;
            }
            match self.try_write(b) {
                Ok(n) => {
                    written += n;
                    // Partial write: stop and let the caller retry.
                    if n < b.len() {
                        break;
                    }
                }
                Err(e) => {
                    // If we already wrote something, surface progress first.
                    if written > 0 {
                        return Ok(written);
                    }
                    return Err(e);
                }
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<()>;

    // === Zero-copy RX token access (optional) ===
    //
    // 设计目标：
    // - bring-up 阶段允许后端通过 `RxToken` 暴露“短生命周期”的只读视图；
    // - 上层（transport/pipeline）可选择立即 materialize（拷贝）为 owned bytes，保证安全；
    // - 后续若要演进到真正的零拷贝（ref-count/arena），可以在不破坏语义的前提下迭代。
    //
    // 重要约束：
    // - 只有当 `try_read_lease()` 返回 `ReadData::Token(tok)` 时 tok 才可能有效；
    // - `rx_ptr_len(tok)` 返回的 ptr/len 仅在 `release_rx(tok)` 之前有效；
    // - 默认实现返回 None，表示该后端不支持 token 视图。

    /// Return a pointer/length view for a leased RX token.
    ///
    /// 默认不支持（返回 None）。支持者需保证：ptr..ptr+len 在 release_rx(tok) 前有效。
    #[inline]
    fn rx_ptr_len(&mut self, _tok: RxToken) -> Option<(*const u8, usize)> {
        None
    }

    /// Release a previously leased RX token.
    ///
    /// 默认 no-op。
    #[inline]
    fn release_rx(&mut self, _tok: RxToken) {}

    /// 关闭底层句柄（语义：close）。
    ///
    /// 设计说明：
    /// - 对齐 Netty/DotNetty：`close()` 是一个明确的 outbound 语义事件，最终应落实到具体 IO 后端。
    /// - 为保持 `no_std` 友好与向后兼容，这里提供默认实现（Unsupported）。
    /// - 具体 transport（TCP/UDP/…）可以覆写实现真正的 shutdown/close。
    #[inline]
    fn close(&mut self) -> Result<()> {
        Err(KernelError::Unsupported)
    }
}

// NOTE:
// 早期版本里曾把 IoOps 叫做 AbstractChannel（偏 OOP 的命名）。
// 为了减少“同义别名”造成的困惑，并避免 deprecated re-export 产生的 warning，
// 我们直接删除该别名 trait。
//
// 若你有旧代码仍在使用 AbstractChannel，请把它替换成 IoOps。
