use super::{MsgBoundary, ReadData};

/// Result metadata for a read operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOutcome {
    /// Number of bytes read.
    pub n: usize,
    /// Boundary semantics.
    pub boundary: MsgBoundary,
    /// Whether the source message/datagram was truncated to fit the destination.
    pub truncated: bool,
    /// Data delivery mode.
    pub data: ReadData,
}
