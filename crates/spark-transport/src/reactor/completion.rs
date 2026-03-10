//! Completion-style reactor foundation (IOCP / io_uring).
//!
//! This module intentionally does **not** change the bring-up dataplane.
//!
//! DECISION (BigStep-17, multi-backend roadmap):
//! - We keep the default transport driver on the **readiness** contract (`Reactor` + `IoOps`).
//! - We still need an explicit place to model **completion** semantics so IOCP/io_uring work can
//!   proceed without contaminating the readiness API surface.
//! - The goal of this module is to define **stable, minimal** types that can be used by leaf
//!   backends (`spark-transport-iocp`, future `spark-transport-uring`) and contract tests.
//!
//! Why keep it minimal:
//! - Completion IO requires additional lifetime/ownership modeling (buffers, op tokens, cancellation).
//! - Stabilizing those semantics prematurely is risky and would slow the "将军赶路" mainline.
//! - We therefore start with a single responsibility: **report completions**.
//!
//! Future work (explicitly out of scope for this step):
//! - Submission APIs (accept/read/write/connect) that safely model buffer ownership.
//! - A completion-driver variant of `AsyncBridge`.

use core::mem::MaybeUninit;

use spark_uci::{Budget, KernelError, Result};

/// Kind of completed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Accept,
    Connect,
    Read,
    Write,
    Close,
}

/// Completion event emitted by a completion backend.
///
/// Note: `outcome` uses `KernelError` to keep cross-platform mapping consistent with the readiness contract suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionEvent {
    pub chan_id: u32,
    pub kind: CompletionKind,
    /// Backend-provided token (opaque to core). Typical usage: (idx, gen) or pointer-tag.
    pub token: u64,
    /// Bytes transferred on success (0 is valid for EOF/close).
    pub outcome: Result<u32>,
}

impl CompletionEvent {
    #[inline]
    pub const fn ok(chan_id: u32, kind: CompletionKind, token: u64, bytes: u32) -> Self {
        Self {
            chan_id,
            kind,
            token,
            outcome: Ok(bytes),
        }
    }

    #[inline]
    pub const fn err(chan_id: u32, kind: CompletionKind, token: u64, err: KernelError) -> Self {
        Self {
            chan_id,
            kind,
            token,
            outcome: Err(err),
        }
    }
}

/// Minimal completion reactor interface.
///
/// DECISION: separate "poll completions" from "submit ops".
/// - Polling is required by all completion backends.
/// - Submission APIs must model buffer ownership and cancellation; we introduce them later
///   once IOCP/io_uring paths converge.
pub trait CompletionReactor {
    /// Poll completion events into a caller-provided buffer.
    fn poll_completions(
        &mut self,
        budget: Budget,
        out: &mut [MaybeUninit<CompletionEvent>],
    ) -> Result<usize>;
}
