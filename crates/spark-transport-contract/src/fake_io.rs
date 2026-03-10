use spark_transport::io::ReadOutcome;
use spark_transport::{KernelError, Result};
use spark_transport::io::{caps, ChannelCaps, IoOps};

/// 用于 contract tests 的“脚本化 IO”实现。
///
/// 设计目标：
/// - 不依赖 OS socket，能稳定复现 WouldBlock / partial write / progress 场景；
/// - 遵循与真实 backend 相同的 `IoOps` 语义（特别是 WouldBlock / Closed）。
pub struct ScriptedIo {
    pub closed: bool,
    /// 允许本轮写出的总字节预算（用于模拟 writable 事件）。
    pub write_allowance: usize,
    /// 统计：try_write 调用次数（用于检测忙等/无进展循环）。
    pub write_calls: u64,
}

impl ScriptedIo {
    pub fn new() -> Self {
        Self {
            closed: false,
            write_allowance: 0,
            write_calls: 0,
        }
    }

    /// 模拟“writable 事件”到来：增加写预算。
    pub fn add_allowance(&mut self, n: usize) {
        self.write_allowance = self.write_allowance.saturating_add(n);
    }
}

impl Default for ScriptedIo {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl IoOps for ScriptedIo {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::WouldBlock)
    }

    fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
        Err(KernelError::WouldBlock)
    }

    fn try_write(&mut self, src: &[u8]) -> Result<usize> {
        self.write_calls = self.write_calls.saturating_add(1);

        if self.closed {
            return Err(KernelError::Closed);
        }
        if self.write_allowance == 0 {
            return Err(KernelError::WouldBlock);
        }

        let n = src.len().min(self.write_allowance).min(1024);
        self.write_allowance -= n;
        Ok(n)
    }

    fn flush(&mut self) -> Result<()> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }
}


impl spark_transport::async_bridge::dyn_channel::DynChannel for ScriptedIo {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
