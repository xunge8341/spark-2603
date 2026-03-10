//! Native completion-style IOCP prototype.
//!
//! This module is **compile-time gated** behind `feature = "native-completion"`.
//!
//! DECISION (BigStep-17):
//! - We keep `spark-dist-iocp` green and contract-compatible by default (readiness wrapper).
//! - In parallel, we build a *native* IOCP completion reactor that can be iterated on without
//!   changing the readiness driver.
//! - This avoids a risky cross-cutting refactor while still moving the IOCP roadmap forward.
//!
//! Current scope:
//! - Provide a minimal `CompletionReactor` that can poll an IO completion port.
//! - Provide a minimal **submission closure** for explicit completion packets (`PostQueuedCompletionStatus`).
//!
//! Why this matters:
//! - True overlapped I/O submission (`AcceptEx` / `WSARecv` / `WSASend`) is the largest remaining gap,
//!   but it also carries the highest semantic risk (buffer ownership, cancellation, partial progress).
//! - Before we wire those APIs into the dataplane, we still need a *real* submit → completion → poll
//!   loop that can run in CI/nightly and that freezes how `CompletionEvent` flows through the IOCP leaf.
//! - Posted completion packets give us that minimum runnable closure without prematurely stabilizing the
//!   full overlapped ownership model.

use core::mem::MaybeUninit;

use spark_transport::reactor::{CompletionEvent, CompletionKind, CompletionReactor};
use spark_uci::{Budget, KernelError, Result};

use std::boxed::Box;
use std::vec::Vec;

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, PostQueuedCompletionStatus, OVERLAPPED,
    OVERLAPPED_ENTRY,
};

/// Low-bit tag used to distinguish our synthetic posted packets from real OVERLAPPED pointers.
///
/// DECISION (BigStep-19): keep submit/poll closure leaf-local and auditable.
/// - `PostQueuedCompletionStatus` stores the pointer value verbatim for posted packets.
/// - Real `OVERLAPPED` pointers are naturally aligned, so the low bit stays 0.
/// - We therefore set bit-0 on our boxed packet pointer, letting `poll_completions` decode and free
///   synthetic packets without guessing about ownership of real kernel-overlapped allocations.
const POSTED_PACKET_TAG: usize = 1;

#[derive(Debug)]
struct PostedCompletionPacket {
    event: CompletionEvent,
}

#[inline]
fn encode_posted_packet_ptr(raw: *mut PostedCompletionPacket) -> *mut OVERLAPPED {
    ((raw as usize) | POSTED_PACKET_TAG) as *mut OVERLAPPED
}

#[inline]
fn is_posted_packet_ptr(ptr: *mut OVERLAPPED) -> bool {
    ((ptr as usize) & POSTED_PACKET_TAG) != 0
}

#[inline]
fn decode_posted_packet_ptr(ptr: *mut OVERLAPPED) -> *mut PostedCompletionPacket {
    ((ptr as usize) & !POSTED_PACKET_TAG) as *mut PostedCompletionPacket
}

/// IOCP completion reactor (prototype).
///
/// Note: This type is *not* yet wired into `AsyncBridge`.
#[derive(Debug)]
pub struct IocpCompletionReactor {
    port: HANDLE,
}

// CreateIoCompletionPort creates a new completion port when:
//   - existingcompletionport == NULL
//   - filehandle == INVALID_HANDLE_VALUE
//
// NOTE: `windows-sys` models HANDLE as `*mut c_void` (not an integer).
// We keep the sentinel value leaf-local to avoid sprinkling Windows-specific casts.
const INVALID_HANDLE_VALUE: HANDLE = (-1isize) as HANDLE;

#[inline]
fn last_error() -> u32 {
    // SAFETY: Win32 FFI. Correctness relies on the standard `GetLastError` contract:
    // it must be called immediately after a failing Win32 API call on the same thread.
    unsafe { GetLastError() }
}

impl IocpCompletionReactor {
    /// Create a new completion port.
    pub fn new() -> core::result::Result<Self, KernelError> {
        // Create an unassociated completion port.
        //
        // DECISION: keep the port lifetime explicit and auditable. `Drop` closes the handle.
        // SAFETY: Win32 FFI. Creating an unassociated completion port is safe here because
        // we pass INVALID_HANDLE_VALUE and NULL existing port, per CreateIoCompletionPort contract.
        let h = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 0) };
        if h.is_null() {
            return Err(KernelError::Internal(last_error()));
        }
        Ok(Self { port: h })
    }

    #[inline]
    pub fn handle(&self) -> HANDLE {
        self.port
    }

    /// Post an explicit completion packet into the port.
    ///
    /// This is the minimal submission closure for the current milestone:
    /// - it makes completion delivery runnable and testable *today*;
    /// - it does **not** yet claim ownership over user buffers or socket operations.
    pub fn post(&self, event: CompletionEvent) -> Result<()> {
        let packet = Box::new(PostedCompletionPacket { event });
        let raw = Box::into_raw(packet);
        let overlapped = encode_posted_packet_ptr(raw);

        let bytes = match event.outcome {
            Ok(bytes) => bytes,
            Err(_) => 0,
        };

        // SAFETY: Win32 FFI. `self.port` is a live completion port handle until Drop, and
        // the overlapped pointer either encodes our posted packet (owned) or is NULL (allowed).
        let ok = unsafe { PostQueuedCompletionStatus(self.port, bytes, event.chan_id as usize, overlapped) };
        if ok == 0 {
            // SAFETY: reclaim the heap allocation created by `Box::into_raw` on the failure path.
            let _ = unsafe { Box::from_raw(raw) };
            return Err(KernelError::Internal(last_error()));
        }

        Ok(())
    }

    #[inline]
    pub fn post_ok(&self, chan_id: u32, kind: CompletionKind, token: u64, bytes: u32) -> Result<()> {
        self.post(CompletionEvent::ok(chan_id, kind, token, bytes))
    }

    #[inline]
    pub fn post_err(&self, chan_id: u32, kind: CompletionKind, token: u64, err: KernelError) -> Result<()> {
        self.post(CompletionEvent::err(chan_id, kind, token, err))
    }
}

impl Drop for IocpCompletionReactor {
    fn drop(&mut self) {
        if !self.port.is_null() {
            // SAFETY: Win32 FFI. Closing a live HANDLE is safe and idempotent with our nulling.
            unsafe {
                CloseHandle(self.port);
            }
            self.port = null_mut();
        }
    }
}

impl CompletionReactor for IocpCompletionReactor {
    fn poll_completions(
        &mut self,
        budget: Budget,
        out: &mut [MaybeUninit<CompletionEvent>],
    ) -> Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        // Clamp the number of entries we ask for to the caller buffer.
        let max = core::cmp::min(out.len(), budget.max_events as usize);

        // Use an uninitialized buffer to avoid redundant zeroing.
        //
        // DECISION: this is leaf-only (iocp backend). Core crates remain `unsafe`-minimal.
        let mut entries: Vec<core::mem::MaybeUninit<OVERLAPPED_ENTRY>> = vec![core::mem::MaybeUninit::uninit(); max];

        let mut removed: u32 = 0;
        let timeout_ms = {
            let ms = budget.max_nanos / 1_000_000;
            if ms > u32::MAX as u64 {
                u32::MAX
            } else {
                ms as u32
            }
        };

        // SAFETY: Win32 FFI. `entries` has capacity `max` and we pass its pointer for the API to
        // initialize up to `removed` entries. We only read the first `removed` items afterwards.
        let ok = unsafe {
            GetQueuedCompletionStatusEx(
                self.port,
                entries.as_mut_ptr() as *mut OVERLAPPED_ENTRY,
                max as u32,
                &mut removed,
                timeout_ms,
                0,
            )
        };

        if ok == 0 {
            // Timeout is not an error: return 0 completions.
            let err = last_error();
            // WAIT_TIMEOUT is 258.
            if err == 258 {
                return Ok(0);
            }
            return Err(KernelError::Internal(err));
        }

        let n = removed as usize;
        for i in 0..n {
            // SAFETY: by GetQueuedCompletionStatusEx contract, the first `removed` entries are initialized.
            let e = unsafe { entries[i].assume_init() };

            let overlapped = e.lpOverlapped;
            if !overlapped.is_null() && is_posted_packet_ptr(overlapped) {
                let raw = decode_posted_packet_ptr(overlapped);
                // SAFETY: `raw` was allocated by `Box::into_raw` in `post`; we recover ownership exactly once.
                let packet = unsafe { Box::from_raw(raw) };
                out[i].write(packet.event);
                continue;
            }

            // CompletionKey carries chan_id in our planned mapping.
            // For now, keep it best-effort: truncation is explicit.
            let chan_id = e.lpCompletionKey as u32;
            let bytes = e.dwNumberOfBytesTransferred as u32;

            // The completion token is currently not encoded for native overlapped operations.
            // Use lpOverlapped pointer value until per-op submit APIs land.
            let token = overlapped as u64;

            // Kind is unknown at this stage; default to Read.
            // True overlapped submission will set kind explicitly.
            let ev = CompletionEvent::ok(chan_id, CompletionKind::Read, token, bytes);
            out[i].write(ev);
        }

        Ok(n)
    }
}
