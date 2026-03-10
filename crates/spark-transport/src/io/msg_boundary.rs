/// Message boundary semantics returned by a read operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgBoundary {
    /// Stream semantics: boundary is unknown / not provided.
    None,
    /// A complete message/datagram was read.
    Complete,
}
