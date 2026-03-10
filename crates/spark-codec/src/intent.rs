/// Codec intent signals to the transport/adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    NeedRead,
    NeedWrite,
    HandshakeStep,
}
