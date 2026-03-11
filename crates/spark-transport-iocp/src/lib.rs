//! `spark-transport-iocp`: Windows IOCP backend integration (dataplane).
//!
//! # Why this crate exists
//! - We need a **Windows-native** backend that can satisfy the same semantic contracts as the mio/epoll/kqueue family.
//! - We also want a **stable leaf boundary** so future backend work does not leak into core semantics.
//!
//! # Phase-0 implementation strategy
//!
//! DECISION (BigStep-11, multi-backend bring-up):
//! - **Do not** change transport-core semantics to completion-style IO yet (that would be a risky cross-cutting refactor).
//! - Instead, we provide an IOCP distribution path that is contract-compatible **today** by delegating to the existing
//!   readiness engine on Windows.
//!
//! Why this is still IOCP-relevant:
//! - On Windows, mio's poller is backed by the platform IOCP-based mechanisms (e.g. `wepoll`).
//! - Wrapping the engine here allows us to:
//!   1) run the full contract+dogfooding pipeline through a Windows-only leaf boundary;
//!   2) freeze public assembly and naming (`spark-dist-iocp`) without polluting core crates;
//!   3) later replace the internals with a **native IOCP completion driver** without changing user-facing hosting.
//!
//! This is a deliberate *bring-up* step to keep the project moving at "将军赶路" pace.
//!
//! Status (baseline 2026-03):
//! - Default dataplane path is still the phase-0 wrapper (not native overlapped socket submission).
//! - Native completion is available only as an opt-in prototype via `native-completion` feature/gate.

use core::mem::MaybeUninit;

use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_transport::reactor::{Interest, KernelEvent, Reactor};
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};
use spark_uci::{Budget, Result};

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

// -----------------------------------------------------------------------------
// Native completion prototype (opt-in)
// -----------------------------------------------------------------------------

#[cfg(all(windows, feature = "native-completion"))]
mod native_completion;

#[cfg(all(windows, feature = "native-completion"))]
pub use native_completion::IocpCompletionReactor;

/// A spawned TCP dataplane instance (IOCP distribution path).
///
/// This mirrors the mio handle shape for embedding and test ergonomics.
#[derive(Debug)]
pub struct TcpDataplaneHandle {
    pub join: thread::JoinHandle<()>,
    pub local_addr: SocketAddr,
}

// === Reactor surface ===

/// Windows IOCP reactor (phase-0 wrapper).
///
/// DECISION: keep the *type name* stable (`IocpReactor`) so downstream code can depend on it,
/// while the internals may evolve from readiness-wrapper to native IOCP completion.
#[cfg(windows)]
#[derive(Debug)]
pub struct IocpReactor {
    inner: spark_transport_mio::MioReactor,
}

#[cfg(windows)]
impl IocpReactor {
    #[inline]
    pub fn new() -> std::io::Result<Self> {
        spark_transport_mio::MioReactor::new().map(|inner| Self { inner })
    }
}

#[cfg(windows)]
impl Reactor for IocpReactor {
    #[inline]
    fn poll_into(&mut self, budget: Budget, out: &mut [MaybeUninit<KernelEvent>]) -> Result<usize> {
        self.inner.poll_into(budget, out)
    }

    #[inline]
    fn register(&mut self, chan_id: u32, interest: Interest) -> core::result::Result<(), spark_uci::KernelError> {
        self.inner.register(chan_id, interest)
    }
}

/// Non-Windows stub (compile-only).
#[cfg(not(windows))]
#[derive(Debug, Default)]
pub struct IocpReactor;

#[cfg(not(windows))]
impl Reactor for IocpReactor {
    fn poll_into(&mut self, _budget: Budget, _out: &mut [MaybeUninit<KernelEvent>]) -> Result<usize> {
        Err(spark_uci::KernelError::Unsupported)
    }

    fn register(&mut self, _chan_id: u32, _interest: Interest) -> core::result::Result<(), spark_uci::KernelError> {
        Err(spark_uci::KernelError::Unsupported)
    }
}

// === Dataplane spawn surface ===

/// Try-spawn a TCP dataplane via the IOCP distribution path.
///
/// DECISION: keep the signature identical to the mio backend. This allows `spark-dist-iocp`
/// to be a true drop-in distribution crate (ASP.NET Core style UX), while the backend impl
/// can evolve independently.
#[cfg(windows)]
pub fn try_spawn_tcp_dataplane<A>(
    cfg: DataPlaneConfig,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    spark_transport_mio::try_spawn_tcp_dataplane(cfg, draining, app, metrics).map(|h| TcpDataplaneHandle {
        join: h.join,
        local_addr: h.local_addr,
    })
}

/// Non-Windows stub.
#[cfg(not(windows))]
pub fn try_spawn_tcp_dataplane<A>(
    _cfg: DataPlaneConfig,
    _draining: Arc<AtomicBool>,
    _app: Arc<A>,
    _metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "spark-transport-iocp is Windows-only",
    ))
}

#[inline]
pub fn spawn_tcp_dataplane<A>(
    cfg: DataPlaneConfig,
    draining: Arc<AtomicBool>,
    app: Arc<A>,
    metrics: Arc<DataPlaneMetrics>,
) -> std::io::Result<TcpDataplaneHandle>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    try_spawn_tcp_dataplane(cfg, draining, app, metrics)
}
