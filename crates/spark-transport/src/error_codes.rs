//! Internal error code registry for `KernelError::Internal(u32)`.
//!
//! Production requirements:
//! - No magic numbers scattered across the codebase.
//! - Stable encoding that keeps enough structure for cross-platform diagnostics.
//!
//! Encoding convention:
//! - Use [`crate::uci::internal_code`] to compose `(subsystem, detail)`.
//! - Subsystem is an 8-bit id; detail is a 24-bit value.

use crate::uci::internal_code;

// Subsystem ids (high 8 bits).
pub const SUBSYS_TRANSPORT: u8 = 0x01;
pub const SUBSYS_MIO: u8 = 0x02;

// OS-level passthrough subsystems.
// - On Unix, detail is `errno` (masked to 24 bits).
// - On Windows, detail is `GetLastError()` / WinSock error (masked to 24 bits).
pub const SUBSYS_OS_ERRNO: u8 = 0x10;
pub const SUBSYS_OS_WIN32: u8 = 0x11;

// -----------------------------------------------------------------------------
// Transport core (0x01)

pub const ERR_LEASE_FAILED: u32 = internal_code(SUBSYS_TRANSPORT, 0x000001);
pub const ERR_IO_UNKNOWN: u32 = internal_code(SUBSYS_TRANSPORT, 0x000002);
pub const ERR_CHAN_SLOT_OCCUPIED: u32 = internal_code(SUBSYS_TRANSPORT, 0x000003);
pub const ERR_FLUSH_UNKNOWN: u32 = internal_code(SUBSYS_TRANSPORT, 0x000004);
pub const ERR_LEASE_CONTRACT_COPIED: u32 = internal_code(SUBSYS_TRANSPORT, 0x000005);
pub const ERR_RX_PTR_LEN_NONE: u32 = internal_code(SUBSYS_TRANSPORT, 0x000006);
pub const ERR_INTERNAL_HTTP1_STATE: u32 = internal_code(SUBSYS_TRANSPORT, 0x000007);
pub const ERR_TASK_STATE_CONFLICT: u32 = internal_code(SUBSYS_TRANSPORT, 0x000008);

// -----------------------------------------------------------------------------
// Mio backend (0x02)

pub const ERR_MIO_POLL_FAILED: u32 = internal_code(SUBSYS_MIO, 0x000001);
pub const ERR_MIO_REGISTER_FAILED: u32 = internal_code(SUBSYS_MIO, 0x000002);
pub const ERR_MIO_ACCEPT_SETUP_FAILED: u32 = internal_code(SUBSYS_MIO, 0x000003);

