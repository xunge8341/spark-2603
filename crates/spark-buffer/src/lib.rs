#![no_std]

//! `spark-buffer` provides the framework's lowest-level byte view types.
//!
//! Design goals:
//! - Keep dependencies minimal (no `bytes` crate).
//! - Provide enough capability for the transport/codec core path.
//! - Remain extensible: advanced pool/zero-copy adapters live in leaf crates.

extern crate alloc;

mod byte_queue;
mod bytes;
mod bytes_mut;
mod cumulation;
mod cursor;
mod endian;
pub mod frame;
mod slice;

pub mod crc;
pub mod hexdump;
pub mod scan;
pub mod serial;
pub mod varint;

pub use byte_queue::ByteQueue;
pub use bytes::Bytes;
pub use bytes_mut::{BytesMut, BytesMutAllocEvidence};
pub use slice::{SliceBuf, SliceBufMut};

pub use cumulation::{Cumulation, CumulationAllocEvidence, TakeStats};

pub use scan::find_byte;

pub use cursor::{Cursor, CursorMut};
pub use endian::*;

pub use hexdump::Hex;
