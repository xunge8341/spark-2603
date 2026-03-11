use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use spark_buffer::Bytes;

use crate::executor::{Executor, RunStatus, TaskRunner};
use crate::reactor::{Interest, KernelEvent, Reactor};
use crate::{Budget, KernelError, Result, RxToken, TaskToken};
use spark_core::service::Service;

use super::pipeline::{AppServiceOptions, FrameDecoderProfile};
use crate::evidence::{EvidenceHandle, EvidenceSink};
use crate::lease::{LeaseRegistry, RxLease};
use crate::metrics::DataPlaneMetrics;
use crate::policy::{FlushBudget, FlushPolicy, WatermarkPolicy};
use crate::slab::Slab;

use super::channel::{Channel, ChannelLimits};
use super::channel_state::tok_chan_id;
use super::dyn_channel::DynChannel;
use super::outbound_buffer::{FlushStatus, WritabilityChange};
use super::task::{TaskSlot, TaskState};
use super::waker::{token_waker, WakeQueue};

// Channel id encoding
//
// We encode (slot_idx, slot_gen) into a single u32 so that:
// - the reactor token is stable per live registration;
// - closed/reclaimed slots can be safely reused without stale events hitting a new channel;
// - the transport core can keep a fixed-size channel table (no unbounded growth).
//
// Layout: [ gen : (32-CHAN_IDX_BITS) | idx : CHAN_IDX_BITS ]
//
// CHAN_IDX_BITS=20 supports up to 1,048,576 concurrent channels (>= 1M).
const CHAN_IDX_BITS: u32 = 20;
const CHAN_IDX_MASK: u32 = (1u32 << CHAN_IDX_BITS) - 1;
const CHAN_GEN_MASK: u32 = (1u32 << (32 - CHAN_IDX_BITS)) - 1;

/// Stable internal vocabulary for driver-side work.
///
/// ADR-013 freezes these semantic buckets first so readiness today and completion tomorrow
/// can converge on the same scheduling kernel before we swap concrete containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleReason {
    InterestSync,
    Reclaim,
    DrainingFlush,
    WriteKick,
    FlushFollowup,
    ReadKick,
}

impl ScheduleReason {
    #[inline]
    const fn bit(self) -> u8 {
        match self {
            ScheduleReason::InterestSync => 1u8 << 0,
            ScheduleReason::Reclaim => 1u8 << 1,
            ScheduleReason::DrainingFlush => 1u8 << 2,
            ScheduleReason::WriteKick => 1u8 << 3,
            ScheduleReason::FlushFollowup => 1u8 << 4,
            ScheduleReason::ReadKick => 1u8 << 5,
        }
    }
}

#[inline]
fn record_schedule_enqueue(metrics: &DataPlaneMetrics, reason: ScheduleReason) {
    match reason {
        ScheduleReason::InterestSync => {
            metrics
                .driver_schedule_interest_sync_total
                .fetch_add(1, Ordering::Relaxed);
        }
        ScheduleReason::Reclaim => {
            metrics
                .driver_schedule_reclaim_total
                .fetch_add(1, Ordering::Relaxed);
        }
        ScheduleReason::DrainingFlush => {
            metrics
                .driver_schedule_draining_flush_total
                .fetch_add(1, Ordering::Relaxed);
        }
        ScheduleReason::WriteKick => {
            metrics
                .driver_schedule_write_kick_total
                .fetch_add(1, Ordering::Relaxed);
        }
        ScheduleReason::FlushFollowup => {
            metrics
                .driver_schedule_flush_followup_total
                .fetch_add(1, Ordering::Relaxed);
        }
        ScheduleReason::ReadKick => {
            metrics
                .driver_schedule_read_kick_total
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Per-slot scheduling state for the current live generation.
///
/// We key de-duplication by full `chan_id` (idx + generation), not bare slot index, so stale work
/// from a reclaimed slot cannot suppress scheduling for the next occupant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ScheduleSlotState {
    chan_id: Option<u32>,
    pending_bits: u8,
}

/// Per-slot task ownership state for the current live generation.
///
/// This freezes the "at most one executor task token per live channel" contract as
/// slot-local, generation-aware state. It lets the scheduling kernel reason in per-channel
/// terms without a global `HashSet<u32>` in the hot path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TaskSlotState {
    chan_id: Option<u32>,
    token: Option<TaskToken>,
}

/// Per-slot reactor interest cache for the current live generation.
///
/// This de-duplicates `reactor.register()` calls and enables edge-triggered backends
/// to avoid redundant syscalls when interests are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InterestSlotState {
    chan_id: Option<u32>,
    interest: Interest,
}

#[inline]
fn registered_interest_on(interest_state: &[InterestSlotState], chan_id: u32) -> Interest {
    let idx = chan_index(chan_id);
    let Some(state) = interest_state.get(idx) else {
        return Interest::empty();
    };
    if state.chan_id == Some(chan_id) {
        state.interest
    } else {
        Interest::empty()
    }
}

#[inline]
fn update_registered_interest_on(
    interest_state: &mut [InterestSlotState],
    chan_id: u32,
    interest: Interest,
) -> bool {
    let idx = chan_index(chan_id);
    let Some(state) = interest_state.get_mut(idx) else {
        return false;
    };
    if state.chan_id != Some(chan_id) {
        *state = InterestSlotState {
            chan_id: Some(chan_id),
            interest,
        };
        return true;
    }
    if state.interest == interest {
        return false;
    }
    state.interest = interest;
    true
}

#[inline]
fn clear_registered_interest_for(interest_state: &mut [InterestSlotState], chan_id: u32) {
    let idx = chan_index(chan_id);
    let Some(state) = interest_state.get_mut(idx) else {
        return;
    };
    if state.chan_id == Some(chan_id) {
        *state = InterestSlotState::default();
    }
}

#[inline]
fn task_is_inflight_on(task_state: &[TaskSlotState], chan_id: u32) -> bool {
    let idx = chan_index(chan_id);
    let Some(state) = task_state.get(idx) else {
        return false;
    };
    state.chan_id == Some(chan_id) && state.token.is_some()
}

#[inline]
fn bind_task_token_on(task_state: &mut [TaskSlotState], chan_id: u32, token: TaskToken) -> bool {
    let idx = chan_index(chan_id);
    let Some(state) = task_state.get_mut(idx) else {
        return false;
    };

    if state.chan_id != Some(chan_id) {
        if state.token.is_some() {
            return false;
        }
        *state = TaskSlotState {
            chan_id: Some(chan_id),
            token: None,
        };
    }

    if state.token.is_some() {
        return false;
    }

    state.token = Some(token);
    true
}

#[inline]
fn take_task_token_on(task_state: &mut [TaskSlotState], chan_id: u32) -> Option<TaskToken> {
    let idx = chan_index(chan_id);
    let state = task_state.get_mut(idx)?;
    if state.chan_id != Some(chan_id) {
        return None;
    }

    let token = state.token.take();
    if state.token.is_none() {
        state.chan_id = None;
    }
    token
}

#[inline]
fn clear_schedule_reason_on(state: &mut ScheduleSlotState, reason: ScheduleReason) {
    state.pending_bits &= !reason.bit();
    if state.pending_bits == 0 {
        state.chan_id = None;
    }
}

#[inline]
fn queue_schedule_reason_on(
    reason: ScheduleReason,
    schedule_state: &mut [ScheduleSlotState],
    queue: &mut Vec<u32>,
    chan_id: u32,
) -> bool {
    // Used when we must keep field-level borrows split (for example while iterating
    // `self.channels.iter_mut()`). The explicit `reason` keeps the kernel vocabulary visible
    // in call sites while the concrete storage stays `Vec<u32>`, and the slot state freezes
    // single-enqueue semantics for the current live generation.
    let idx = chan_index(chan_id);
    let Some(state) = schedule_state.get_mut(idx) else {
        return false;
    };

    if state.chan_id != Some(chan_id) {
        *state = ScheduleSlotState {
            chan_id: Some(chan_id),
            pending_bits: 0,
        };
    }

    let bit = reason.bit();
    if (state.pending_bits & bit) != 0 {
        return false;
    }

    state.pending_bits |= bit;
    queue.push(chan_id);
    true
}

#[inline]
fn take_schedule_batch(
    reason: ScheduleReason,
    schedule_state: &mut [ScheduleSlotState],
    queue: &mut Vec<u32>,
    max_per_tick: usize,
) -> (Vec<u32>, u64) {
    if queue.is_empty() || max_per_tick == 0 {
        return (Vec::new(), 0);
    }

    let todo = core::mem::take(queue);
    let mut ready = Vec::with_capacity(core::cmp::min(todo.len(), max_per_tick));
    let mut rest = Vec::new();
    let mut stale_skipped: u64 = 0;

    for chan_id in todo {
        let idx = chan_index(chan_id);
        let Some(state) = schedule_state.get_mut(idx) else {
            stale_skipped += 1;
            continue;
        };
        if state.chan_id != Some(chan_id) {
            stale_skipped += 1;
            continue;
        }
        if ready.len() < max_per_tick {
            clear_schedule_reason_on(state, reason);
            ready.push(chan_id);
        } else {
            rest.push(chan_id);
        }
    }

    *queue = rest;
    (ready, stale_skipped)
}

/// Internal helper for leaf integration crates (mio/uring/etc) and contract tests.
///
/// A `chan_id` is an encoded (slot_idx, slot_gen) value; this returns the slot index.
///
/// Design note:
/// - We intentionally expose the *stable* name `chan_index` (no leading underscores)
///   to keep cross-crate call sites clean and consistent.
/// - This helper is *not* part of the user-facing dataplane API surface.
#[inline]
#[doc(hidden)]
pub fn chan_index(chan_id: u32) -> usize {
    (chan_id & CHAN_IDX_MASK) as usize
}

#[inline]
fn chan_pack(idx: u32, gen: u32) -> u32 {
    ((gen & CHAN_GEN_MASK) << CHAN_IDX_BITS) | (idx & CHAN_IDX_MASK)
}

#[inline]
fn chan_next_gen(gen: u32) -> u32 {
    let mut g = (gen.wrapping_add(1)) & CHAN_GEN_MASK;
    if g == 0 {
        g = 1;
    }
    g
}

// NOTE: Do not add alias/compat helpers here. We keep a single canonical name.

/// Back-compat name: "AsyncBridge" from earlier bring-up.
///
/// Dev-friendly defaults:
/// - `Ev` defaults to `Arc<dyn EvidenceSink>` (runtime-neutral)
/// - `Io` defaults to `Box<dyn DynChannel>` (bring-up friendly)
pub type AsyncBridge<R, E, A, Ev = Arc<dyn EvidenceSink>, Io = Box<dyn DynChannel>> =
    ChannelDriver<R, E, A, Ev, Io>;

/// Construction limits for `ChannelDriver`.
///
/// This groups closely related numeric knobs to keep constructor signatures short and
/// developer-friendly under `-D warnings`.
#[derive(Debug, Clone, Copy)]
pub struct DriverLimits {
    pub max_channels: usize,
    pub max_frame: usize,
}

/// Construction config for [`ChannelDriver`].
///
/// This groups the less frequently modified knobs to keep public constructor
/// signatures short and clippy-clean.
#[derive(Debug, Clone, Copy)]
pub struct DriverConfig {
    pub watermark: WatermarkPolicy,
    pub framing: FrameDecoderProfile,
    pub flush_policy: FlushPolicy,
}

impl DriverConfig {
    #[inline]
    pub fn new(
        watermark: WatermarkPolicy,
        framing: FrameDecoderProfile,
        flush_policy: FlushPolicy,
    ) -> Self {
        Self {
            watermark,
            framing,
            flush_policy,
        }
    }
}

/// 单线程数据面 driver（对齐 Netty 的 EventLoop 角色）。
///
/// 职责边界（关键点）：
/// - driver 只做：poll reactor -> dispatch -> poll app futures -> flush outbound -> sync interest。
/// - 每连接的语义状态（cumulation/decoder/outbound buffer/watermarks/pipeline）都在 `Channel<A>` 内部。
///
/// 这是为了达到你要求的“像 Netty 一样概念清晰”：
/// Reactor/Engine 只产生事件；EventLoop 只驱动；Channel 才是语义对象。
pub struct ChannelDriver<R, E, A, Ev = Arc<dyn EvidenceSink>, Io = Box<dyn DynChannel>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    reactor: R,
    executor: E,
    app: Arc<A>,

    slab: Slab<TaskSlot>,
    channels: Vec<Option<Channel<A, Ev, Io>>>,

    // Per-slot task ownership state (see ADR-013 / docs/DRIVER_SCHEDULING_KERNEL.md).
    //
    // The one-task-per-channel contract is now carried by generation-aware slot-local state
    // instead of a global `HashSet<u32>`. This keeps the ownership boundary explicit and lets
    // stale generations fall out naturally when slots are reclaimed and reused.
    task_state: Vec<TaskSlotState>,

    // Per-slot cached reactor interests (best-effort).
    //
    // This reduces redundant `register()` calls in steady state and enables edge-triggered
    // backends to detect interest transitions without a driver-wide scan.
    interest_state: Vec<InterestSlotState>,

    // copy-path 读缓冲（lease 不支持时使用）。
    read_buf: Vec<u8>,
    max_frame: usize,
    framing: FrameDecoderProfile,

    wake_q: Arc<WakeQueue>,
    free: Vec<u32>,
    slot_gen: Vec<u32>,

    high_watermark: usize,
    low_watermark: usize,
    max_pending_write_bytes: usize,
    flush_budget: FlushBudget,
    app_service_opts: AppServiceOptions,

    // Default per-channel draining timeout.
    //
    // Leaf backends that build the driver from `DataPlaneConfig` should override this with
    // `cfg.drain_timeout` to keep shutdown and peer-EOF behavior consistent.
    drain_timeout: std::time::Duration,

    // Driver scheduling kernel (current concrete encoding).
    //
    // These queues remain split by semantic reason, but ADR-013 now also requires single-enqueue
    // semantics per `(chan_id, reason)` before we swap hot-path containers. The per-slot
    // `schedule_state` tracks which reasons are already armed for the live generation in a slot,
    // so queue storage can stay a simple append-only `Vec<u32>` without `sort()` / `dedup()`.
    schedule_state: Vec<ScheduleSlotState>,

    // `FlushFollowup`: channels that hit flush fairness budget and need another bounded flush.
    pending_flush: Vec<u32>,

    // `ReadKick`: channels that exited backpressure and need one bounded read re-entry.
    pending_read_kick: Vec<u32>,

    // Hot-path scratch buffers (avoid per-tick allocations).
    // `InterestSync`: channels whose desired reactor interest must be re-registered.
    scratch_interest_sync: Vec<u32>,
    // `Reclaim`: channels that have reached a reclaimable terminal state.
    scratch_reclaim: Vec<u32>,
    // `DrainingFlush`: draining channels that still require bounded outbound progress.
    scratch_draining_flush: Vec<u32>,
    // `WriteKick`: non-draining channels that still have outbound backlog.
    scratch_write_kick: Vec<u32>,

    metrics: Arc<DataPlaneMetrics>,

    evidence: Ev,
}

impl<R, E, A, Ev, Io> ChannelDriver<R, E, A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    pub fn new_with_policy(
        reactor: R,
        executor: E,
        app: Arc<A>,
        limits: DriverLimits,
        metrics: Arc<DataPlaneMetrics>,
        evidence: Ev,
        policy: WatermarkPolicy,
    ) -> Self {
        let max_frame = limits.max_frame.max(1);
        Self::new_with_config(
            reactor,
            executor,
            app,
            limits,
            metrics,
            evidence,
            DriverConfig::new(
                policy,
                FrameDecoderProfile::line(max_frame),
                FlushPolicy::default(),
            ),
        )
    }

    pub fn new_with_config(
        reactor: R,
        executor: E,
        app: Arc<A>,
        limits: DriverLimits,
        metrics: Arc<DataPlaneMetrics>,
        evidence: Ev,
        config: DriverConfig,
    ) -> Self {
        let max_frame = limits.max_frame.max(1);
        debug_assert!(
            config.framing.max_frame_hint() <= max_frame,
            "framing profile expects max_frame_hint={} but driver max_frame={} (increase config)",
            config.framing.max_frame_hint(),
            max_frame
        );
        let (high, low) = config.watermark.watermarks(max_frame);
        let flush_budget = config.flush_policy.budget(max_frame);

        let max_channels = limits.max_channels.max(1);
        assert!(
            max_channels <= (1u32 << CHAN_IDX_BITS) as usize,
            "max_channels={} exceeds encoding limit ({}). Increase CHAN_IDX_BITS or lower config",
            max_channels,
            (1u32 << CHAN_IDX_BITS)
        );
        let channels: Vec<Option<Channel<A, Ev, Io>>> = (0..max_channels).map(|_| None).collect();
        let free: Vec<u32> = (0..max_channels as u32).rev().collect();
        let slot_gen: Vec<u32> = vec![1u32; max_channels];

        Self {
            reactor,
            executor,
            app,
            slab: Slab::new(max_channels),
            channels,
            free,
            slot_gen,
            task_state: vec![TaskSlotState::default(); max_channels],
            interest_state: vec![InterestSlotState::default(); max_channels],
            read_buf: vec![0u8; max_frame],
            max_frame,
            framing: config.framing,
            wake_q: Arc::new(WakeQueue::default()),
            high_watermark: high,
            low_watermark: low,
            max_pending_write_bytes: usize::MAX,
            flush_budget,
            app_service_opts: AppServiceOptions::default(),
            drain_timeout: std::time::Duration::from_secs(5),
            schedule_state: vec![ScheduleSlotState::default(); max_channels],
            pending_flush: Vec::with_capacity(64),
            pending_read_kick: Vec::with_capacity(64),
            scratch_interest_sync: Vec::with_capacity(64),
            scratch_reclaim: Vec::with_capacity(64),
            scratch_draining_flush: Vec::with_capacity(64),
            scratch_write_kick: Vec::with_capacity(128),
            metrics,
            evidence,
        }
    }

    /// Construct a driver from a runtime-neutral [`DataPlaneConfig`].
    ///
    /// This is the preferred constructor for leaf backends (mio/epoll/iocp/etc)
    /// because it keeps configuration centralized and avoids signature drift.
    pub fn new_with_dataplane_config(
        reactor: R,
        executor: E,
        app: Arc<A>,
        cfg: &crate::DataPlaneConfig,
        metrics: Arc<DataPlaneMetrics>,
        evidence: Ev,
    ) -> Self {
        let limits = DriverLimits {
            max_channels: cfg.max_channels,
            max_frame: cfg.max_frame_hint(),
        };
        let config = DriverConfig::new(cfg.watermark, cfg.framing, cfg.flush_policy);
        let mut driver =
            Self::new_with_config(reactor, executor, app, limits, metrics, evidence, config);
        driver.drain_timeout = cfg.drain_timeout;
        driver.max_pending_write_bytes = cfg.max_pending_write_bytes;
        driver.app_service_opts = cfg.app_service_options();
        driver
    }

    pub fn new(
        reactor: R,
        executor: E,
        app: Arc<A>,
        max_channels: usize,
        max_frame: usize,
        metrics: Arc<DataPlaneMetrics>,
        evidence: Ev,
    ) -> Self {
        Self::new_with_policy(
            reactor,
            executor,
            app,
            DriverLimits {
                max_channels,
                max_frame,
            },
            metrics,
            evidence,
            WatermarkPolicy::default(),
        )
    }

    /// Construct a driver with an explicit framing profile.
    ///
    /// This is intended for internal profiles (e.g. management-plane HTTP/1) and dogfooding.
    pub fn new_with_framing(
        reactor: R,
        executor: E,
        app: Arc<A>,
        limits: DriverLimits,
        metrics: Arc<DataPlaneMetrics>,
        evidence: Ev,
        framing: FrameDecoderProfile,
    ) -> Self {
        Self::new_with_config(
            reactor,
            executor,
            app,
            limits,
            metrics,
            evidence,
            DriverConfig::new(WatermarkPolicy::default(), framing, FlushPolicy::default()),
        )
    }

    pub fn reactor_mut(&mut self) -> &mut R {
        &mut self.reactor
    }

    /// Internal hook for backend integration crates (mio/uring/etc).
    ///
    /// This allows a backend to safely borrow the reactor and channel table at the same time
    /// (e.g. to apply deferred interest updates) without exposing transport internals elsewhere.
    #[doc(hidden)]
    pub fn __with_reactor_and_channels<T>(
        &mut self,
        f: impl FnOnce(&mut R, &mut Vec<Option<Channel<A, Ev, Io>>>) -> T,
    ) -> T {
        let (reactor, channels) = (&mut self.reactor, &mut self.channels);
        f(reactor, channels)
    }

    pub fn alloc_chan_id(&mut self) -> Result<u32> {
        let Some(idx) = self.free.pop() else {
            return Err(KernelError::NoMem);
        };
        let gen = *self.slot_gen.get(idx as usize).unwrap_or(&1u32);
        // bump generation for the next reuse
        if let Some(g) = self.slot_gen.get_mut(idx as usize) {
            *g = chan_next_gen(gen);
        }
        Ok(chan_pack(idx, gen))
    }

    /// Internal hook for leaf integration crates: release an allocated channel id
    /// that was never installed into the channel table.
    ///
    /// This is used in accept/connect error paths to prevent leaking capacity.
    ///
    /// Contract:
    /// - The slot corresponding to `chan_id` must still be empty.
    /// - This function is idempotent (best-effort).
    #[doc(hidden)]
    pub fn __release_uninstalled_chan_id(&mut self, chan_id: u32) {
        let idx = chan_index(chan_id);
        if idx >= self.channels.len() {
            return;
        }
        if self.channels[idx].is_some() {
            return;
        }
        let slot = idx as u32;
        if self.free.contains(&slot) {
            return;
        }
        self.free.push(slot);
    }

    pub fn install_channel(&mut self, chan_id: u32, chan: Io) -> Result<()>
    where
        R: Reactor,
    {
        let idx = chan_index(chan_id);
        if idx >= self.channels.len() {
            return Err(KernelError::Invalid);
        }
        if self.channels[idx].is_some() {
            // Should never happen: chan_id is allocated from the free list.
            return Err(KernelError::Internal(
                crate::error_codes::ERR_CHAN_SLOT_OCCUPIED,
            ));
        }

        let limits = ChannelLimits::new(
            self.max_frame,
            self.high_watermark,
            self.low_watermark,
            self.max_pending_write_bytes,
        );
        let ch = match self.framing {
            FrameDecoderProfile::Line { .. } => Channel::new_with_flush_budget(
                chan_id,
                chan,
                limits,
                self.flush_budget,
                Arc::clone(&self.app),
                self.evidence.clone(),
                self.app_service_opts,
            ),
            _ => Channel::new_with_profile_and_flush_budget(
                chan_id,
                chan,
                self.framing,
                limits,
                self.flush_budget,
                Arc::clone(&self.app),
                self.evidence.clone(),
                self.app_service_opts,
            ),
        };
        if let Some(state) = self.task_state.get_mut(idx) {
            if state.token.is_some() && state.chan_id != Some(chan_id) {
                self.metrics
                    .driver_task_state_conflict_total
                    .fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::Internal(
                    crate::error_codes::ERR_TASK_STATE_CONFLICT,
                ));
            }
            *state = TaskSlotState {
                chan_id: Some(chan_id),
                token: None,
            };
        }
        self.channels[idx] = Some(ch);

        self.metrics
            .active_connections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Contract: installing a live channel must establish an initial reactor interest.
        // Some contract-suite tests mutate channel state (e.g. enter draining) before the first
        // tick; without an initial register, the edge-triggered InterestSync path may observe
        // `prev == desired == empty` and skip the required register call.
        self.sync_interest(chan_id);
        Ok(())
    }

    /// Test/diagnostics helper only: `Any` downcast is FORBIDDEN in hot path (tick/read/flush).
    ///
    /// NOTE: keep it behind `diagnostics` (or tests) to prevent OOP-style probing in prod.
    #[cfg(any(test, feature = "diagnostics"))]
    pub fn channel_mut<T: std::any::Any>(&mut self, chan_id: u32) -> Option<&mut T> {
        let Some(Some(ch)) = self.channels.get_mut(chan_index(chan_id)) else {
            return None;
        };
        ch.io_mut().as_any_mut().downcast_mut::<T>()
    }

    fn drain_wake_queue(&mut self, budget: Budget) -> Result<usize>
    where
        E: Executor,
    {
        let mut n = 0usize;
        let limit = budget.max_events.max(1) as usize;
        while n < limit {
            let Some(tok) = self.wake_q.pop() else {
                break;
            };
            self.executor.submit(tok, 128)?;
            n += 1;
        }
        Ok(n)
    }

    #[inline]
    fn queue_schedule_reason(&mut self, reason: ScheduleReason, chan_id: u32) -> bool {
        let enqueued = match reason {
            ScheduleReason::InterestSync => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.scratch_interest_sync,
                chan_id,
            ),
            ScheduleReason::Reclaim => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.scratch_reclaim,
                chan_id,
            ),
            ScheduleReason::DrainingFlush => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.scratch_draining_flush,
                chan_id,
            ),
            ScheduleReason::WriteKick => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.scratch_write_kick,
                chan_id,
            ),
            ScheduleReason::FlushFollowup => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.pending_flush,
                chan_id,
            ),
            ScheduleReason::ReadKick => queue_schedule_reason_on(
                reason,
                &mut self.schedule_state,
                &mut self.pending_read_kick,
                chan_id,
            ),
        };
        if enqueued {
            record_schedule_enqueue(&self.metrics, reason);
        }
        enqueued
    }

    #[inline]
    fn clear_schedule_state_for(&mut self, chan_id: u32) {
        let idx = chan_index(chan_id);
        let Some(state) = self.schedule_state.get_mut(idx) else {
            return;
        };
        if state.chan_id == Some(chan_id) {
            *state = ScheduleSlotState::default();
        }
    }

    /// Shared READ task submission gate for raw `Readable` edges and `ReadKick` follow-up work.
    ///
    /// `missing_is_paused=false` preserves the old best-effort raw-event behavior (a stale/missing
    /// slot may still yield a task token that self-cleans when driven). `missing_is_paused=true` is
    /// stricter and is used for synthetic follow-up work so we never manufacture reads on reclaimed slots.
    fn try_submit_read_task(&mut self, chan_id: u32, missing_is_paused: bool) -> Result<()>
    where
        E: Executor,
    {
        if task_is_inflight_on(&self.task_state, chan_id) {
            self.metrics
                .driver_task_submit_inflight_suppressed_total
                .fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        let idx = chan_index(chan_id);
        let paused = self
            .channels
            .get(idx)
            .and_then(|c| c.as_ref())
            .filter(|c| c.chan_id() == chan_id)
            .map(|c| c.is_read_paused())
            .unwrap_or(missing_is_paused);
        if paused {
            self.metrics
                .driver_task_submit_paused_suppressed_total
                .fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(tok) = self.slab.insert(TaskSlot {
            chan_id,
            state: TaskState::New,
        }) {
            self.metrics
                .driver_task_submit_total
                .fetch_add(1, Ordering::Relaxed);
            // Bind task ownership before submit so slot reuse stays keyed by the full `chan_id`.
            // If this fails, we must roll back immediately rather than silently shadowing another
            // generation's live token.
            if !bind_task_token_on(&mut self.task_state, chan_id, tok) {
                self.metrics
                    .driver_task_state_conflict_total
                    .fetch_add(1, Ordering::Relaxed);
                self.slab.free(tok);
                return Err(KernelError::Internal(
                    crate::error_codes::ERR_TASK_STATE_CONFLICT,
                ));
            }
            if let Err(err) = self.executor.submit(tok, 128) {
                self.metrics
                    .driver_task_submit_failed_total
                    .fetch_add(1, Ordering::Relaxed);
                // Submission failure must leave both the slab and task-ownership state clean.
                let released = take_task_token_on(&mut self.task_state, chan_id);
                debug_assert_eq!(released, Some(tok));
                self.slab.free(tok);
                return Err(err);
            }
        }
        Ok(())
    }

    fn sync_interest(&mut self, chan_id: u32)
    where
        R: Reactor,
    {
        let idx = chan_index(chan_id);
        let interest = self
            .channels
            .get(idx)
            .and_then(|c| c.as_ref())
            .filter(|c| c.chan_id() == chan_id)
            .map(|c| c.desired_interest())
            .unwrap_or_default();

        if !update_registered_interest_on(&mut self.interest_state, chan_id, interest) {
            self.metrics
                .driver_interest_register_skipped_total
                .fetch_add(1, Ordering::Relaxed);
            return;
        }

        self.metrics
            .driver_interest_register_total
            .fetch_add(1, Ordering::Relaxed);
        let _ = self.reactor.register(chan_id, interest);
    }

    fn flush_outbound(&mut self, chan_id: u32) -> Result<()>
    where
        R: Reactor,
    {
        let idx = chan_index(chan_id);
        let Some(Some(ch)) = self.channels.get_mut(idx) else {
            return Ok(());
        };
        if ch.chan_id() != chan_id {
            // Stale event / reused slot: ignore.
            return Ok(());
        }

        let (status, wrote, syscalls, writev_calls) = ch.flush_outbound();
        self.metrics.record_write(wrote, syscalls, writev_calls);

        // Watermark-driven writability changes: trigger pipeline event.
        match ch.take_writability_change() {
            WritabilityChange::None => {}
            WritabilityChange::BecameUnwritable { .. } => {
                ch.fire_writability_changed(ch.is_writable());
            }
            WritabilityChange::BecameWritable { .. } => {
                ch.fire_writability_changed(ch.is_writable());
                // Queue one bounded `ReadKick` so backpressure exit does not depend on another
                // backend READ edge being delivered.
                self.queue_schedule_reason(ScheduleReason::ReadKick, chan_id);
            }
        }

        self.sync_interest(chan_id);

        match status {
            FlushStatus::Drained | FlushStatus::WouldBlock => Ok(()),
            FlushStatus::Limited => {
                self.metrics.record_flush_limited();
                // Queue a `FlushFollowup` reason; the bounded follow-up happens in the driver tick,
                // not inline here. This keeps the semantic reason explicit and reviewable.
                self.queue_schedule_reason(ScheduleReason::FlushFollowup, chan_id);
                Ok(())
            }
            FlushStatus::Closed => Err(KernelError::Closed),
            FlushStatus::Error => Err(KernelError::Internal(crate::error_codes::ERR_FLUSH_UNKNOWN)),
        }
    }
}

impl<R, E, A, Ev, Io> ChannelDriver<R, E, A, Ev, Io>
where
    R: Reactor,
    E: Executor,
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    pub fn tick(&mut self, budget: Budget) -> Result<()> {
        // 1) Poll events.
        let mut out = [core::mem::MaybeUninit::<KernelEvent>::uninit(); 256];
        let n = self.reactor.poll_into(budget, &mut out)?;
        for ev in crate::reactor::iter_init(&out, n) {
            match ev {
                KernelEvent::Readable { chan_id } => {
                    self.try_submit_read_task(chan_id, false)?;
                }
                KernelEvent::Writable { chan_id } => {
                    // Writable：立即尝试 drain outbound（非阻塞）。
                    let _ = self.flush_outbound(chan_id);
                }
                KernelEvent::Closed { chan_id } => {
                    self.reclaim_channel(chan_id);
                }
                KernelEvent::Timer { .. } => {}
            }
        }

        // 2) Drive tasks.
        //
        // Rust 借用拆分：executor 只借用 `&mut self.executor`；runner 只借用 driver 的其它字段。
        // 这样可以避免 `&mut self` 同时作为 runner + executor 的借用冲突，也避免任何 `unsafe`。
        {
            let mut runner = DriverRunner {
                reactor: &mut self.reactor,
                slab: &mut self.slab,
                channels: &mut self.channels,
                task_state: &mut self.task_state,
                interest_state: &mut self.interest_state,
                read_buf: &mut self.read_buf,
                wake_q: &self.wake_q,
                schedule_state: &mut self.schedule_state,
                pending_flush: &mut self.pending_flush,
                pending_read_kick: &mut self.pending_read_kick,
                metrics: &self.metrics,
                drain_timeout: self.drain_timeout,
            };
            self.executor.drive(&mut runner, budget)?;
        }

        // 3) Drive tasks woken by futures.
        let woken = self.drain_wake_queue(budget)?;
        if woken > 0 {
            {
                let mut runner = DriverRunner {
                    reactor: &mut self.reactor,
                    slab: &mut self.slab,
                    channels: &mut self.channels,
                    task_state: &mut self.task_state,
                    interest_state: &mut self.interest_state,
                    read_buf: &mut self.read_buf,
                    wake_q: &self.wake_q,
                    schedule_state: &mut self.schedule_state,
                    pending_flush: &mut self.pending_flush,
                    pending_read_kick: &mut self.pending_read_kick,
                    metrics: &self.metrics,
                    drain_timeout: self.drain_timeout,
                };
                self.executor.drive(&mut runner, budget)?;
            }
        }

        // 4) Draining / close teardown (channel-level).
        //
        // ADR-013 note:
        // This block is the current concrete implementation of the driver scheduling kernel.
        // Review future changes in terms of *work reasons* first (`InterestSync`, `DrainingFlush`,
        // `WriteKick`, `FlushFollowup`, `ReadKick`, `Reclaim`), and only then in terms of local
        // container/data-structure changes. That keeps readiness today and completion tomorrow
        // aligned behind the same semantic boundary.
        //
        // 说明：
        // - draining 仅在 ops/控制面触发；这里采用线性扫描，优先保证语义正确与证据事件完整。
        // - close_requested 的 no-backlog 连接会被视为“可回收”，本 tick 直接回收资源，
        //   避免依赖 reactor 再次报 Closed（在 deregister 后可能永远收不到）。
        //
        // 注意：遍历 self.channels.iter_mut() 时不能在循环体内调用 self.sync_interest()，
        // 因为 sync_interest 需要 &mut self，会与 iter_mut() 对 self.channels 的可变借用冲突。
        // 这里先收集待同步/待回收的 channel_id，循环结束后统一处理（避免 E0499）。
        self.scratch_interest_sync.clear();
        self.scratch_reclaim.clear();
        self.scratch_draining_flush.clear();
        self.scratch_write_kick.clear();
        {
            let task_state = &self.task_state;
            let metrics = &self.metrics;
            let interest_state = &self.interest_state;
            let (
                channels,
                schedule_state,
                scratch_interest_sync,
                scratch_reclaim,
                scratch_draining_flush,
                scratch_write_kick,
            ) = (
                &mut self.channels,
                &mut self.schedule_state,
                &mut self.scratch_interest_sync,
                &mut self.scratch_reclaim,
                &mut self.scratch_draining_flush,
                &mut self.scratch_write_kick,
            );

            for slot in channels.iter_mut() {
                let Some(ch) = slot.as_mut() else {
                    continue;
                };

                let chan_id = ch.chan_id();
                let prev = registered_interest_on(interest_state, chan_id);

                if ch.is_draining() {
                    let inflight = if task_is_inflight_on(task_state, chan_id) {
                        1
                    } else {
                        0
                    };
                    ch.poll_draining(inflight);
                }

                let desired = ch.desired_interest();

                // Interest sync is now edge-triggered on the effective desired interest.
                // This avoids redundant `register()` calls while keeping cross-backend semantics stable.
                if desired != prev
                    && queue_schedule_reason_on(
                        ScheduleReason::InterestSync,
                        schedule_state,
                        scratch_interest_sync,
                        chan_id,
                    )
                {
                    record_schedule_enqueue(metrics, ScheduleReason::InterestSync);
                }

                if ch.is_draining() {
                    // Cross-platform contract: flushing must make progress even if the reactor
                    // does not emit further Writable events (common after peer half-close).
                    //
                    // If the channel still has outbound backlog, attempt a best-effort flush
                    // from the driver tick.
                    if desired.contains(Interest::WRITE)
                        && queue_schedule_reason_on(
                            ScheduleReason::DrainingFlush,
                            schedule_state,
                            scratch_draining_flush,
                            chan_id,
                        )
                    {
                        record_schedule_enqueue(metrics, ScheduleReason::DrainingFlush);
                    }
                } else {
                    // Non-draining TX progress is kicked only on WRITE-interest edges.
                    //
                    // Why:
                    // - steady-state TX already calls `flush_outbound()` on app completion and on
                    //   reactor Writable events;
                    // - edge-triggered backends may not emit a Writable edge when WRITE interest is
                    //   enabled on an already-writable socket;
                    // - a single kick on the transition to WRITE is sufficient to avoid stalls
                    //   without busy flushing every tick.
                    if desired.contains(Interest::WRITE)
                        && !prev.contains(Interest::WRITE)
                        && queue_schedule_reason_on(
                            ScheduleReason::WriteKick,
                            schedule_state,
                            scratch_write_kick,
                            chan_id,
                        )
                    {
                        record_schedule_enqueue(metrics, ScheduleReason::WriteKick);
                    }
                }

                if ch.is_close_requested()
                    && desired.is_empty()
                    && queue_schedule_reason_on(
                        ScheduleReason::Reclaim,
                        schedule_state,
                        scratch_reclaim,
                        chan_id,
                    )
                {
                    record_schedule_enqueue(metrics, ScheduleReason::Reclaim);
                }
            }
        }

        let (todo, stale) = take_schedule_batch(
            ScheduleReason::InterestSync,
            &mut self.schedule_state,
            &mut self.scratch_interest_sync,
            usize::MAX,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            self.sync_interest(chan_id);
        }

        let (todo, stale) = take_schedule_batch(
            ScheduleReason::Reclaim,
            &mut self.schedule_state,
            &mut self.scratch_reclaim,
            usize::MAX,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            self.reclaim_channel(chan_id);
        }

        // Best-effort flush for draining channels (bounded).
        const MAX_DRAINING_FLUSH_PER_TICK: usize = 64;
        let (todo, stale) = take_schedule_batch(
            ScheduleReason::DrainingFlush,
            &mut self.schedule_state,
            &mut self.scratch_draining_flush,
            MAX_DRAINING_FLUSH_PER_TICK,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            let _ = self.flush_outbound(chan_id);
        }

        // Best-effort flush for non-draining channels with outbound backlog (bounded).
        //
        // Cross-platform note: some backends/platforms may miss Writable transitions (e.g. after
        // backpressure interest toggles). A bounded proactive flush ensures forward progress without
        // busy-spinning.
        const MAX_WRITE_KICK_PER_TICK: usize = 64;
        let (todo, stale) = take_schedule_batch(
            ScheduleReason::WriteKick,
            &mut self.schedule_state,
            &mut self.scratch_write_kick,
            MAX_WRITE_KICK_PER_TICK,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            let _ = self.flush_outbound(chan_id);
        }

        // 5) Follow-up flush: handle channels that hit flush fairness budget.
        const MAX_FLUSH_AGAIN_PER_TICK: usize = 64;
        let (todo, stale) = take_schedule_batch(
            ScheduleReason::FlushFollowup,
            &mut self.schedule_state,
            &mut self.pending_flush,
            MAX_FLUSH_AGAIN_PER_TICK,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            let _ = self.flush_outbound(chan_id);
        }

        // 6) Read kick: when backpressure exits, proactively schedule a READ task.
        //
        // Cross-platform note: some backends are edge-triggered (or may drop READ events when interests
        // are toggled). If we paused reads due to outbound backpressure and later re-enabled READ,
        // we must kick a bounded read to guarantee forward progress.
        const MAX_READ_KICK_PER_TICK: usize = 64;
        let (todo, stale) = take_schedule_batch(
            ScheduleReason::ReadKick,
            &mut self.schedule_state,
            &mut self.pending_read_kick,
            MAX_READ_KICK_PER_TICK,
        );
        if stale > 0 {
            self.metrics
                .driver_schedule_stale_skipped_total
                .fetch_add(stale, Ordering::Relaxed);
        }
        for chan_id in todo {
            // `ReadKick` is stricter than a raw reactor Readable edge: stale/missing channels
            // are treated as paused so we never manufacture work on a reclaimed slot.
            self.try_submit_read_task(chan_id, true)?;
        }
        Ok(())
    }
}

impl<R, E, A, Ev, Io> ChannelDriver<R, E, A, Ev, Io>
where
    R: Reactor,
    E: Executor,
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    fn reclaim_channel(&mut self, chan_id: u32) {
        // Best-effort deregister to avoid reactor busy loop.
        // De-duplicate syscalls via the per-slot interest cache.
        if update_registered_interest_on(&mut self.interest_state, chan_id, Interest::empty()) {
            self.metrics
                .driver_interest_register_total
                .fetch_add(1, Ordering::Relaxed);
            let _ = self.reactor.register(chan_id, Interest::empty());
        } else {
            self.metrics
                .driver_interest_register_skipped_total
                .fetch_add(1, Ordering::Relaxed);
        }

        // Free task token if present. Keep this keyed by full `chan_id` so stale slot generations
        // cannot accidentally free a new occupant's token.
        if let Some(tok) = take_task_token_on(&mut self.task_state, chan_id) {
            self.metrics
                .driver_task_reclaim_total
                .fetch_add(1, Ordering::Relaxed);
            self.slab.free(tok);
        }

        let idx = chan_index(chan_id);
        if idx >= self.channels.len() {
            return;
        }

        // Drop channel only if the slot still matches this encoded chan_id (stale events are ignored).
        let should_drop = self.channels[idx]
            .as_ref()
            .map(|c| c.chan_id() == chan_id)
            .unwrap_or(false);
        if !should_drop {
            return;
        }

        if let Some(mut ch) = self.channels[idx].take() {
            ch.on_inactive();
        }
        self.clear_schedule_state_for(chan_id);
        clear_registered_interest_for(&mut self.interest_state, chan_id);
        if let Some(state) = self.task_state.get_mut(idx) {
            if state.chan_id == Some(chan_id) {
                *state = TaskSlotState::default();
            }
        }

        // Reclaim slot for future allocations.
        self.free.push(idx as u32);

        // Metrics: mirror the Closed path (best-effort).
        self.metrics
            .closed_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = self.metrics.active_connections.fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |v| Some(v.saturating_sub(1)),
        );
    }
}

impl<R, E, A, Ev, Io> LeaseRegistry for ChannelDriver<R, E, A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    fn borrow_rx<'a>(&'a mut self, tok: RxToken) -> Result<RxLease<'a, Self>>
    where
        Self: Sized,
    {
        let chan_id = tok_chan_id(tok);
        let idx = chan_index(chan_id);
        let Some(Some(ch)) = self.channels.get_mut(idx) else {
            return Err(KernelError::Invalid);
        };
        if ch.chan_id() != chan_id {
            return Err(KernelError::Invalid);
        }

        // 通过 IoOps 的 token-view（可选）接口做 lease。
        // 说明：
        // - 语义层不再 downcast 到具体后端（TCP/UDP/…），避免 runtime/transport 绑定；
        // - 后端若不支持 token-view，会返回 None，上层可回退到 copy path。
        let Some((ptr, len)) = ch.io_mut().rx_ptr_len(tok) else {
            return Err(KernelError::Unsupported);
        };
        let reg = self as *mut Self;
        Ok(RxLease::new(ptr, len, tok, reg))
    }

    fn release_rx(&mut self, tok: RxToken) {
        let chan_id = tok_chan_id(tok);
        let idx = chan_index(chan_id);
        if let Some(Some(ch)) = self.channels.get_mut(idx) {
            if ch.chan_id() != chan_id {
                return;
            }
            ch.io_mut().release_rx(tok);
        }
    }
}

struct DriverRunner<'a, R, A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    reactor: &'a mut R,
    slab: &'a mut Slab<TaskSlot>,
    channels: &'a mut Vec<Option<Channel<A, Ev, Io>>>,
    task_state: &'a mut Vec<TaskSlotState>,
    interest_state: &'a mut Vec<InterestSlotState>,
    read_buf: &'a mut Vec<u8>,
    wake_q: &'a Arc<WakeQueue>,
    schedule_state: &'a mut Vec<ScheduleSlotState>,
    pending_flush: &'a mut Vec<u32>,
    pending_read_kick: &'a mut Vec<u32>,
    metrics: &'a Arc<DataPlaneMetrics>,

    drain_timeout: std::time::Duration,
}

impl<R, A, Ev, Io> DriverRunner<'_, R, A, Ev, Io>
where
    R: Reactor,
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    fn sync_interest(&mut self, chan_id: u32) {
        let idx = chan_index(chan_id);
        let interest = self
            .channels
            .get(idx)
            .and_then(|c| c.as_ref())
            .filter(|c| c.chan_id() == chan_id)
            .map(|c| c.desired_interest())
            .unwrap_or_default();

        if !update_registered_interest_on(self.interest_state, chan_id, interest) {
            self.metrics
                .driver_interest_register_skipped_total
                .fetch_add(1, Ordering::Relaxed);
            return;
        }

        self.metrics
            .driver_interest_register_total
            .fetch_add(1, Ordering::Relaxed);
        let _ = self.reactor.register(chan_id, interest);
    }

    #[inline]
    fn finish_task(&mut self, chan_id: u32, token: TaskToken) {
        // Tasks release ownership by exact `(chan_id, token)` pairing so stale wakeups cannot
        // clear a newer generation's slot-local state.
        let released = take_task_token_on(self.task_state, chan_id);
        debug_assert_eq!(released, Some(token));
        self.metrics
            .driver_task_finish_total
            .fetch_add(1, Ordering::Relaxed);
        self.slab.free(token);
    }

    #[inline]
    fn queue_schedule_reason(&mut self, reason: ScheduleReason, chan_id: u32) -> bool {
        let enqueued = match reason {
            ScheduleReason::FlushFollowup => {
                queue_schedule_reason_on(reason, self.schedule_state, self.pending_flush, chan_id)
            }
            ScheduleReason::ReadKick => queue_schedule_reason_on(
                reason,
                self.schedule_state,
                self.pending_read_kick,
                chan_id,
            ),
            _ => {
                debug_assert!(
                    false,
                    "runner only owns follow-up work queues: {:?}",
                    reason
                );
                false
            }
        };
        if enqueued {
            record_schedule_enqueue(self.metrics.as_ref(), reason);
        }
        enqueued
    }

    fn flush_outbound(&mut self, chan_id: u32) -> Result<()> {
        let idx = chan_index(chan_id);
        let Some(Some(ch)) = self.channels.get_mut(idx) else {
            return Ok(());
        };
        if ch.chan_id() != chan_id {
            return Ok(());
        }

        let (status, wrote, syscalls, writev_calls) = ch.flush_outbound();
        self.metrics.record_write(wrote, syscalls, writev_calls);

        match ch.take_writability_change() {
            WritabilityChange::None => {}
            WritabilityChange::BecameUnwritable { .. } => {
                ch.fire_writability_changed(ch.is_writable());
            }
            WritabilityChange::BecameWritable { .. } => {
                ch.fire_writability_changed(ch.is_writable());
                // Queue one bounded `ReadKick` so backpressure exit does not depend on another
                // backend READ edge being delivered.
                self.queue_schedule_reason(ScheduleReason::ReadKick, chan_id);
            }
        }

        self.sync_interest(chan_id);

        match status {
            FlushStatus::Drained | FlushStatus::WouldBlock => Ok(()),
            FlushStatus::Limited => {
                self.metrics.record_flush_limited();
                // Queue a `FlushFollowup` reason; the bounded follow-up happens in the driver tick,
                // not inline here. This keeps the semantic reason explicit and reviewable.
                self.queue_schedule_reason(ScheduleReason::FlushFollowup, chan_id);
                Ok(())
            }
            FlushStatus::Closed => Err(KernelError::Closed),
            FlushStatus::Error => Err(KernelError::Internal(crate::error_codes::ERR_FLUSH_UNKNOWN)),
        }
    }
}

impl<R, A, Ev, Io> TaskRunner for DriverRunner<'_, R, A, Ev, Io>
where
    R: Reactor,
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceHandle,
    Io: DynChannel,
{
    fn run_token(&mut self, token: TaskToken) -> RunStatus {
        // 将 TaskSlot 从 slab 中临时取出：这样在处理过程中可以自由地 re-enter `&mut self`
        //（例如调用 flush_outbound / sync_interest 等），避免任何裸指针或借用黑魔法。
        let Some(mut slot) = self.slab.take(token) else {
            return RunStatus::Done;
        };

        let chan_id = slot.chan_id;
        let idx = chan_index(chan_id);

        // small step-loop：避免一次 token 内跑太久。
        for _ in 0..3 {
            // 将 state 临时取出，避免在持有 `&mut fut` 时又整体覆盖 state（borrow 重入）。
            let state = core::mem::replace(&mut slot.state, TaskState::New);

            match state {
                TaskState::New => {
                    let ch = match self.channels.get_mut(idx) {
                        Some(Some(ch)) if ch.chan_id() == chan_id => ch,
                        _ => {
                            self.finish_task(chan_id, token);
                            return RunStatus::Closed;
                        }
                    };

                    if let Some(fut) = ch.take_app_future() {
                        slot.state = TaskState::App { fut };
                        continue;
                    }

                    // Read + decode loop（bounded）。
                    const MAX_READS_PER_CALL: usize = 8;
                    let (
                        read_bytes,
                        decoded,
                        decode_errs,
                        too_large,
                        coalesce_count,
                        copied_bytes,
                        cumulation_copy_bytes,
                        lease_tokens,
                        lease_borrowed_bytes,
                        materialize_bytes,
                    ) = match ch.on_readable(&mut self.read_buf[..], MAX_READS_PER_CALL) {
                        Ok(v) => v,
                        Err(KernelError::WouldBlock) => (0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
                        Err(KernelError::Closed) => {
                            // P0 contract: deterministically transition to close so the driver can reclaim the slot.
                            // Metrics are accounted in `reclaim_channel` to avoid double-counting.
                            ch.force_close_on_error(KernelError::Closed);
                            self.finish_task(chan_id, token);
                            return RunStatus::Closed;
                        }
                        Err(err) => {
                            // P0 contract: any I/O error must deterministically transition to close.
                            // This prevents backend-specific "stuck" channels and yields stable evidence.
                            ch.force_close_on_error(err);
                            self.finish_task(chan_id, token);
                            return RunStatus::Closed;
                        }
                    };

                    // Peer half-close (EOF) must not abort pending writes.
                    // Enter draining so the channel will close after flushing responses.
                    if ch.peer_eof() && !ch.is_draining() {
                        ch.enter_draining(true, self.drain_timeout, 1);
                    }

                    self.metrics.record_read(read_bytes);
                    self.metrics.record_decode(decoded, decode_errs, too_large);
                    self.metrics
                        .record_rx_lease(lease_tokens, lease_borrowed_bytes);
                    self.metrics.record_rx_materialize(materialize_bytes);
                    self.metrics
                        .record_inbound_cumulation(coalesce_count, copied_bytes);
                    self.metrics
                        .record_rx_cumulation_copy(cumulation_copy_bytes);

                    if let Some(fut) = ch.take_app_future() {
                        slot.state = TaskState::App { fut };
                        continue;
                    }

                    // 继续保持 New（后续事件再驱动）。
                    slot.state = TaskState::New;

                    // 没有完整消息 / 没有业务 future。
                    self.finish_task(chan_id, token);
                    return RunStatus::Done;
                }

                TaskState::App { mut fut } => {
                    let waker = token_waker(Arc::clone(self.wake_q), token);
                    let mut cx = TaskContext::from_waker(&waker);
                    match fut.as_mut().poll(&mut cx) {
                        Poll::Pending => {
                            // 还原 state。
                            slot.state = TaskState::App { fut };
                            debug_assert!(self.slab.put(token, slot));
                            return RunStatus::Done;
                        }
                        Poll::Ready(res) => {
                            // 注意：这里需要同时做两件事：
                            // 1) 回调 pipeline（可能 enqueue 响应 / 生成下一条业务 future / 写入 backlog）。
                            // 2) best-effort flush outbound。
                            // 但 flush_outbound() 需要再次可变借用 self.channels，因此不能在持有 ch 的可变借用时调用。
                            // 解决方式：先在一个短作用域内完成对 ch 的修改并取出 next/backlog 标志，再释放借用后 flush。

                            let (next_fut, has_backlog) = {
                                let ch = match self.channels.get_mut(idx) {
                                    Some(Some(ch)) if ch.chan_id() == chan_id => ch,
                                    _ => {
                                        self.finish_task(chan_id, token);
                                        return RunStatus::Closed;
                                    }
                                };

                                // 回调 pipeline，写回响应/启动下一条。
                                ch.on_app_complete(res);

                                // 取出下一条 future（如果已就绪）。
                                let next = ch.take_app_future();
                                let backlog = ch.has_app_backlog();
                                (next, backlog)
                            };

                            // writabilityChanged/backpressure 指标由 flush_outbound() 统一处理（包含 enqueue 后的水位线变化）。
                            // best-effort flush（non-blocking）。
                            let _ = self.flush_outbound(chan_id);

                            // 若 service handler 已经准备好下一条 future，立即继续。
                            if let Some(next) = next_fut {
                                slot.state = TaskState::App { fut: next };
                                continue;
                            }

                            // 或者还有 backlog（但尚未启动 inflight）——保守起见回到 New 让它启动。
                            if has_backlog {
                                slot.state = TaskState::New;
                                continue;
                            }

                            // 无后续工作：state 回到 New（不再持有 fut）。
                            slot.state = TaskState::New;

                            self.finish_task(chan_id, token);
                            return RunStatus::Done;
                        }
                    }
                }
            }
        }

        // 走到这里意味着 small step-loop 用尽预算：把 slot 放回去等待后续事件/唤醒。
        debug_assert!(self.slab.put(token, slot));
        RunStatus::Done
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bind_task_token_on, chan_pack, queue_schedule_reason_on, take_schedule_batch,
        take_task_token_on, ScheduleReason, ScheduleSlotState, TaskSlotState,
    };
    use crate::TaskToken;

    #[test]
    fn queue_schedule_reason_on_dedups_same_reason_per_live_channel() {
        let chan_id = chan_pack(7, 1);
        let mut state = vec![ScheduleSlotState::default(); 8];
        let mut queue = Vec::new();

        assert!(queue_schedule_reason_on(
            ScheduleReason::WriteKick,
            &mut state,
            &mut queue,
            chan_id,
        ));
        assert!(!queue_schedule_reason_on(
            ScheduleReason::WriteKick,
            &mut state,
            &mut queue,
            chan_id,
        ));
        assert_eq!(queue, vec![chan_id]);
    }

    #[test]
    fn take_schedule_batch_clears_only_the_requested_reason() {
        let chan_id = chan_pack(3, 1);
        let mut state = vec![ScheduleSlotState::default(); 4];
        let mut write_queue = Vec::new();
        let mut read_queue = Vec::new();

        assert!(queue_schedule_reason_on(
            ScheduleReason::WriteKick,
            &mut state,
            &mut write_queue,
            chan_id,
        ));
        assert!(queue_schedule_reason_on(
            ScheduleReason::ReadKick,
            &mut state,
            &mut read_queue,
            chan_id,
        ));

        let (todo, _stale) =
            take_schedule_batch(ScheduleReason::WriteKick, &mut state, &mut write_queue, 8);
        assert_eq!(todo, vec![chan_id]);
        assert_eq!(read_queue, vec![chan_id]);

        let (todo, _stale) =
            take_schedule_batch(ScheduleReason::ReadKick, &mut state, &mut read_queue, 8);
        assert_eq!(todo, vec![chan_id]);
        assert_eq!(state[3], ScheduleSlotState::default());
    }

    #[test]
    fn take_schedule_batch_skips_stale_generation_and_keeps_new_one() {
        let old_chan_id = chan_pack(2, 1);
        let new_chan_id = chan_pack(2, 2);
        let mut state = vec![ScheduleSlotState::default(); 4];
        let mut queue = Vec::new();

        assert!(queue_schedule_reason_on(
            ScheduleReason::FlushFollowup,
            &mut state,
            &mut queue,
            old_chan_id,
        ));
        assert!(queue_schedule_reason_on(
            ScheduleReason::FlushFollowup,
            &mut state,
            &mut queue,
            new_chan_id,
        ));

        let (todo, _stale) =
            take_schedule_batch(ScheduleReason::FlushFollowup, &mut state, &mut queue, 1);
        assert_eq!(todo, vec![new_chan_id]);
        assert!(queue.is_empty());
    }

    #[test]
    fn bind_and_take_task_token_tracks_live_channel() {
        let chan_id = chan_pack(1, 7);
        let mut state = vec![TaskSlotState::default(); 4];
        let token = TaskToken { idx: 11, gen: 3 };

        assert!(bind_task_token_on(&mut state, chan_id, token));
        assert!(!bind_task_token_on(&mut state, chan_id, token));
        assert_eq!(take_task_token_on(&mut state, chan_id), Some(token));
        assert_eq!(state[1], TaskSlotState::default());
    }

    #[test]
    fn bind_task_token_rejects_cross_generation_conflict() {
        let old_chan_id = chan_pack(2, 1);
        let new_chan_id = chan_pack(2, 2);
        let mut state = vec![TaskSlotState::default(); 4];

        assert!(bind_task_token_on(
            &mut state,
            old_chan_id,
            TaskToken { idx: 5, gen: 1 },
        ));
        assert!(!bind_task_token_on(
            &mut state,
            new_chan_id,
            TaskToken { idx: 6, gen: 1 },
        ));
        assert_eq!(state[2].chan_id, Some(old_chan_id));
    }
}
