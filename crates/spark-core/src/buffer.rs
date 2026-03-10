/// Minimal buffer abstraction for L0.
///
/// Implementations:
/// - `spark-buffer` provides safe slice-based buffers.
/// - server-side byte buffer adapters live outside L0.
pub trait Buffer {
    fn as_slice(&self) -> &[u8];
}

pub trait BufferMut: Buffer {
    fn as_mut_slice(&mut self) -> &mut [u8];
}
