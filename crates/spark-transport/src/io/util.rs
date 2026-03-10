use spark_uci::{KernelError, Result};

/// Convenience helper for stubs.
#[inline]
pub fn unsupported<T>() -> Result<T> {
    Err(KernelError::Unsupported)
}
