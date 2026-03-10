/// Capability bitflags for channels / IO backends.
pub type ChannelCaps = u32;

/// Stream semantics.
pub const STREAM: u32 = 1 << 0;
/// Datagram semantics.
pub const DATAGRAM: u32 = 1 << 1;
/// Reliable delivery.
pub const RELIABLE: u32 = 1 << 2;
/// In-order delivery.
pub const ORDERED: u32 = 1 << 3;
/// RX zero-copy supported.
pub const ZERO_COPY_RX: u32 = 1 << 4;
/// TX zero-copy supported.
pub const ZERO_COPY_TX: u32 = 1 << 5;
