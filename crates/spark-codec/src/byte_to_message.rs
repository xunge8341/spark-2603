use spark_core::context::Context;

/// Outcome of an incremental stream decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeOutcome<T> {
    /// Not enough input to produce a full message.
    NeedMore,
    /// A message was produced; `consumed` bytes should be removed from cumulation.
    Message { consumed: usize, message: T },
}

/// Netty-style incremental decoder for byte streams.
///
/// This is the Rust equivalent of Netty's `ByteToMessageDecoder` contract:
///
/// - The adapter owns a **cumulation buffer** (bytes already read but not yet decoded).
/// - Each time new bytes arrive, the adapter appends to cumulation, then calls `decode`
///   in a loop until it returns `NeedMore`.
/// - The decoder may produce *multiple* messages from a single cumulation.
///
/// Design notes:
/// - We keep the signature `no_std` friendly (no `BytesMut`), and model consumption
///   explicitly via `consumed`.
/// - Backends can layer in zero-copy (slices/leases) by making `message` a view type.
pub trait ByteToMessageDecoder {
    type Error;
    type Message;

    fn decode(
        &mut self,
        ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error>;
}
