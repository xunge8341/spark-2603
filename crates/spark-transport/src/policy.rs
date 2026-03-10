//! Runtime policies.
//!
//! 设计目标：
//! - **开发者友好**：默认策略覆盖 99% 使用场景；高级用户按需显式注入。
//! - **可测试/可回归**：策略本身尽量是纯函数/小对象，便于 contract tests 固化行为。
//! - **像 sqlx/embedded-hal**：核心语义稳定；不同运行时/平台差异通过少量策略对象表达。

use crate::io::{AcceptDecision, ConnectDecision};

/// Accept error policy.
///
/// 默认策略：调用 `io::classify_accept_error`。
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptPolicy;

impl AcceptPolicy {
    #[inline]
    pub fn decide(&self, e: &std::io::Error) -> AcceptDecision {
        crate::io::classify_accept_error(e)
    }
}

/// Nonblocking connect error policy.
///
/// 默认策略：调用 `io::classify_connect_error`。
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectPolicy;

impl ConnectPolicy {
    #[inline]
    pub fn decide(&self, e: &std::io::Error) -> ConnectDecision {
        crate::io::classify_connect_error(e)
    }
}

/// Watermark policy used by the outbound buffer/backpressure.
///
/// 默认策略（bring-up 版本）：
/// - base = max(max_frame, min_frame)
/// - high = base * high_mul
/// - low  = base * low_mul
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatermarkPolicy {
    pub min_frame: usize,
    pub high_mul: usize,
    pub low_mul: usize,
}

impl Default for WatermarkPolicy {
    fn default() -> Self {
        Self {
            min_frame: 1024,
            high_mul: 8,
            low_mul: 4,
        }
    }
}

impl WatermarkPolicy {
    #[inline]
    pub fn watermarks(&self, max_frame: usize) -> (usize, usize) {
        let base = max_frame.max(self.min_frame);
        let high = base.saturating_mul(self.high_mul);
        let low = base.saturating_mul(self.low_mul);
        (high, low)
    }
}

/// Maximum number of iov entries we will ever build on the stack for a single write call.
///
/// DECISION: keep iovec arrays stack-only and predictable on all platforms.
/// This cap is enforced in `FlushPolicy::budget()` (clamp) and used by `OutboundBuffer` for allocation-free batching.
pub const MAX_IOV_CAP: usize = 16;

/// Per-flush fairness budget.
///
/// Motivation:
/// - Avoid one hot connection monopolizing a tick when the socket is always writable.
/// - Keep semantics deterministic across reactors (edge/level triggered).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlushBudget {
    /// Maximum bytes to write per `flush_outbound()` call.
    pub max_bytes: usize,
    /// Maximum syscalls to attempt per `flush_outbound()` call.
    pub max_syscalls: usize,
    /// Maximum iov entries to batch per syscall (clamped to `MAX_IOV_CAP`).
    ///
    /// DECISION: expose batching shape as a product knob, but keep an upper bound to preserve
    /// stack-only iovec construction and stable p99/p999 in production.
    pub max_iov: usize,
}

impl FlushBudget {
    /// Construct a flush budget for tests and synthetic drivers.
    ///
    /// DECISION (API stability): `FlushBudget` is `#[non_exhaustive]` to prevent brittle struct
    /// literals across crates. Use this constructor (and `with_max_iov`) so future budget knobs
    /// can be added without breaking call sites.
    #[inline]
    pub const fn new(max_bytes: usize, max_syscalls: usize) -> Self {
        Self {
            max_bytes,
            max_syscalls,
            // Default batching shape; still clamped by the outbound buffer to `MAX_IOV_CAP`.
            max_iov: 8,
        }
    }

    /// Override the preferred maximum iov entries per syscall.
    ///
    /// DECISION: this is a throughput vs CPU knob; we keep it explicit at call sites and clamp
    /// again at the actual flush boundary.
    #[inline]
    pub const fn with_max_iov(mut self, max_iov: usize) -> Self {
        self.max_iov = max_iov;
        self
    }
}

impl Default for FlushBudget {
    fn default() -> Self {
        // Conservative default; production code should normally use `FlushPolicy::budget()`.
        Self::new(1, 1).with_max_iov(1)
    }
}

/// Flush fairness policy used by the outbound buffer.
///
/// 默认策略（bring-up 版本）：
/// - base = max(max_frame, min_frame)
/// - max_bytes = min(base * max_bytes_mul, max_bytes_cap)
/// - max_syscalls = max_syscalls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlushPolicy {
    pub min_frame: usize,
    pub max_bytes_mul: usize,
    pub max_bytes_cap: usize,
    pub max_syscalls: usize,
    /// Preferred maximum iov entries per write syscall.
    pub max_iov: usize,
}

impl Default for FlushPolicy {
    fn default() -> Self {
        Self {
            // For small frames, keep a reasonable baseline.
            min_frame: 64 * 1024,
            // Default: ~2 MiB per flush when max_frame is 64 KiB.
            max_bytes_mul: 32,
            // Hard cap to avoid pathological monopolization.
            max_bytes_cap: 16 * 1024 * 1024,
            // A small syscall cap; vectored writes reduce syscall pressure.
            max_syscalls: 32,
            // Default: small stack iovec; can be increased for throughput profiles.
            max_iov: 8,
        }
    }
}

impl FlushPolicy {
    #[inline]
    pub fn budget(&self, max_frame: usize) -> FlushBudget {
        let base = max_frame.max(self.min_frame);
        let max_bytes = base
            .saturating_mul(self.max_bytes_mul)
            .min(self.max_bytes_cap)
            .max(1);
        let max_syscalls = self.max_syscalls.max(1);
        let max_iov = self.max_iov.clamp(1, MAX_IOV_CAP);
        FlushBudget {
            max_bytes,
            max_syscalls,
            max_iov,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AcceptPolicy, ConnectPolicy, FlushPolicy, WatermarkPolicy};

    #[test]
    fn watermark_policy_is_sane() {
        let p = WatermarkPolicy::default();
        let (h, l) = p.watermarks(1024);
        assert!(h > l);
    }

    #[test]
    fn flush_policy_is_sane() {
        let p = FlushPolicy::default();
        let b = p.budget(64 * 1024);
        assert!(b.max_bytes > 0);
        assert!(b.max_syscalls > 0);
        assert!(b.max_iov > 0);
    }

    #[test]
    fn policies_are_constructible() {
        let _a = AcceptPolicy;
        let _c = ConnectPolicy;
    }
}
