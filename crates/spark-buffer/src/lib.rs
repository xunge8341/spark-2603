#![no_std]

//! `spark-buffer` provides the framework's lowest-level byte view types.
//!
//! Design goals:
//! - Keep dependencies minimal (no `bytes` crate).
//! - Provide enough capability for the transport/codec core path.
//! - Remain extensible: advanced pool/zero-copy adapters live in leaf crates.

extern crate alloc;

mod bytes;
mod bytes_mut;
mod byte_queue;
mod slice;
mod endian;
mod cursor;
mod cumulation;
pub mod frame;

pub mod crc;
pub mod hexdump;
pub mod serial;
pub mod varint;
pub mod scan;

pub use bytes::Bytes;
pub use bytes_mut::BytesMut;
pub use byte_queue::ByteQueue;
pub use slice::{SliceBuf, SliceBufMut};

pub use cumulation::{Cumulation, TakeStats};

pub use scan::find_byte;

pub use cursor::{Cursor, CursorMut};
pub use endian::*;

pub use hexdump::Hex;

