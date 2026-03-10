//! IOCP native completion prototype smoke test.
//!
//! DECISION (BigStep-19):
//! - The completion prototype must validate a *real* submit → completion → poll loop, not only port
//!   creation + empty polling.
//! - We still avoid true overlapped socket ownership here (`AcceptEx` / `WSARecv` / `WSASend`): those
//!   remain a separate semantic surface and will be gated by dedicated dataplane/contract suites.
//! - This smoke therefore covers the minimum runnable closure we can safely harden now:
//!   1) IOCP port creation
//!   2) explicit completion packet submission (`PostQueuedCompletionStatus`)
//!   3) stable `CompletionEvent` round-trip through `poll_completions(..)`

#![cfg(all(windows, feature = "native-completion"))]

use core::mem::MaybeUninit;
use spark_transport::reactor::{CompletionEvent, CompletionKind, CompletionReactor};
use spark_uci::{Budget, KernelError};

use spark_transport_iocp::IocpCompletionReactor;

#[test]
fn native_completion_port_can_be_created_and_polled() {
    let mut rx = IocpCompletionReactor::new().expect("create iocp completion port");

    // Poll with 0ns timeout; should return quickly with 0 events.
    let budget = Budget { max_events: 8, max_nanos: 0 };
    let mut out: [MaybeUninit<CompletionEvent>; 8] = [MaybeUninit::uninit(); 8];
    let n = CompletionReactor::poll_completions(&mut rx, budget, &mut out).expect("poll completions");
    assert_eq!(n, 0);
}

#[test]
fn native_completion_posted_packets_roundtrip_explicit_events() {
    let mut rx = IocpCompletionReactor::new().expect("create iocp completion port");

    let ev_ok = CompletionEvent::ok(7, CompletionKind::Accept, 0xAA55, 123);
    let ev_err = CompletionEvent::err(11, CompletionKind::Write, 0xBB66, KernelError::Closed);

    rx.post(ev_ok).expect("post ok completion");
    rx.post(ev_err).expect("post err completion");

    let budget = Budget { max_events: 8, max_nanos: 0 };
    let mut out: [MaybeUninit<CompletionEvent>; 8] = [MaybeUninit::uninit(); 8];
    let n = CompletionReactor::poll_completions(&mut rx, budget, &mut out).expect("poll completions");
    assert_eq!(n, 2);

    let first = unsafe { out[0].assume_init() };
    let second = unsafe { out[1].assume_init() };

    let pair = [first, second];
    assert!(pair.iter().any(|ev| *ev == ev_ok), "missing posted ok completion: {pair:?}");
    assert!(pair.iter().any(|ev| *ev == ev_err), "missing posted err completion: {pair:?}");
}
