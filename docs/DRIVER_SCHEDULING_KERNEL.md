# Driver Scheduling Kernel Plan

This document is the trunk-facing plan for the next foundation step in `spark-transport`.
It intentionally focuses on the **driver kernel** rather than local micro-optimizations.

## Why this comes next
The driver is the semantic absorption layer between:
- backend event delivery (mio / readiness today),
- future completion delivery (IOCP / io_uring style), and
- stable channel semantics (flush progress, read progress, close / reclaim).

If the scheduling shape is not explicit, later performance work can become a local optimum that must be redone.

## Design constraints
This plan is explicitly constrained by:
- **ADR-000**: no runtime-polymorphic hot-path drift.
- **ADR-003**: no long-lived compatibility state.
- **ADR-010**: backend differences stop at the driver boundary.
- **ADR-012**: completion submission stays out of trunk until its ownership/contracts are ready.

## Scope
The near-term scope is to make the scheduling kernel explicit and reviewable.

### In scope
- Make driver-side work reasons explicit and stable.
- Keep current behavior and contracts intact.
- Add internal observability for scheduler pressure and follow-up work.
- Prepare the code for a later swap from `HashSet + Vec` to per-channel flags once single-enqueue semantics are frozen in code.

### Out of scope
- No user-visible semantic changes.
- No broad RX zero-copy redesign yet.
- No io_uring / WASI expansion.
- No readiness-to-completion rewrite in the trunk dataplane.

## Kernel vocabulary
The driver should internally think in these work reasons:
- `InterestSync`
- `DrainingFlush`
- `WriteKick`
- `FlushFollowup`
- `ReadKick`
- `Reclaim`

These are semantic categories, not a promise about the concrete container type.

## Execution plan

### Step A — Record the semantic kernel
- Leave decision records in ADR + iteration log.
- Annotate the current queues in code as the concrete encoding of those work reasons.
- Make future reviews compare semantics first, data structures second.

### Step B — Add internal observability (now in trunk)
We add **stable, label-free** internal counters to explain scheduler/task pressure without changing user-visible semantics.

**Scheduler pressure**
- `driver_schedule_interest_sync_total`
- `driver_schedule_reclaim_total`
- `driver_schedule_draining_flush_total`
- `driver_schedule_write_kick_total`
- `driver_schedule_flush_followup_total`
- `driver_schedule_read_kick_total`
- `driver_schedule_stale_skipped_total` (stale queue items dropped before consuming bounded per-tick budget)

**Reactor register de-dup**
- `driver_interest_register_total`
- `driver_interest_register_skipped_total`

**Task ownership pressure**
- `driver_task_submit_total`
- `driver_task_submit_failed_total`
- `driver_task_submit_inflight_suppressed_total`
- `driver_task_submit_paused_suppressed_total`
- `driver_task_finish_total`
- `driver_task_reclaim_total`
- `driver_task_state_conflict_total`

These names are anchored in `spark_uci::names::metrics` and exported by `spark-metrics-prometheus` as `spark_dp_*`.


### Step C — Freeze single-enqueue semantics
- Define which work reasons may co-exist per channel per tick.
- Define which reasons collapse into one queue entry and which must remain distinct.
- Preserve boundedness requirements.
- **Step C1 (now in trunk):** make the vocabulary explicit in code via an internal `ScheduleReason` enum and queue helpers while keeping the current `Vec`-backed storage unchanged.
- **Step C2 (now in trunk):** freeze single-enqueue semantics per `(chan_id, reason)` using generation-aware slot state, so `Vec` queues can stay append-only and bounded without `sort()` / `dedup()`, while stale items are dropped before they consume bounded per-tick work budget.
- **Step C3 (now in trunk):** freeze the one-task-per-channel contract as generation-aware slot-local state (`TaskSlotState`) so task ownership no longer depends on a global `HashSet<u32>`. Reclaim now releases the exact recorded task token for the live `chan_id`, which avoids slow-path slab scans and keeps slot reuse rules explicit.
- **Step C4 (now in trunk):** make interest synchronization edge-triggered on effective desired-interest changes and de-duplicate `reactor.register()` calls via a generation-aware per-slot cache (`InterestSlotState`). Non-draining `WriteKick` is now scheduled on WRITE-interest *edges* (transition to WRITE) to avoid busy flushing every tick while still preventing edge-triggered stalls.

### Step D — Replace hot-path containers
Only after A/B/C are stable:
- keep the new explicit single-enqueue behavior,
- replace global `HashSet<u32>` checks where per-channel state is sufficient,
- keep external behavior unchanged.

### Step E — Revisit RX zero-copy
After the scheduling kernel and metrics are stable:
- use the new evidence to decide how far to push leased RX views,
- keep unsafe confined,
- avoid broad lifetime churn while completion submission is still evolving.

## Review checklist
When reviewing driver changes, ask in this order:
1. Does this preserve the scheduling reasons?
2. Does this preserve bounded forward progress?
3. Does this keep backend differences inside the driver boundary?
4. Does this improve observability / auditability?
5. Only then: does it reduce CPU / allocation / syscall cost?

## Contract note: initial interest baseline (Step C4 follow-up)
Edge-triggered interest sync introduces a subtle correctness dependency:
- contract tests may transition a channel into draining/close **before** the first driver tick,
- and the effective desired interest may already be `empty` at that time.

If the driver only registers on edges (`desired != last_registered`), a newly installed channel
with an uninitialized cache can observe `prev == desired == empty` and skip the register call,
violating ordering/draining contracts.

**Therefore:** `install_channel()` establishes an initial interest register baseline by calling
`sync_interest(chan_id)` after slot installation.
