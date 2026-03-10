# ADR-013: Freeze the Driver Scheduling Kernel before hot-path micro-optimization

## Status
Accepted (BigStep-28 in progress)

## Context
The trunk is now green with:
- P0 contract suites,
- panic-free / unsafe audit gates,
- symmetric outbound framing,
- framing scan-state cleanup,
- a runnable IOCP completion prototype gate.

The next visible pressure points are:
- the driver scheduling hot path (`HashSet<u32>` + multiple `Vec<u32>` queues + `sort_unstable()` / `dedup()`), and
- the RX lease path, which still materializes leased bytes into owned buffers before decode.

However, several accepted decisions constrain the order of operations:
- **ADR-000**: keep hot paths statically dispatched and keep backend-specific differences out of the core surface.
- **ADR-003**: avoid long-lived intermediate states and compatibility shims.
- **ADR-010**: absorb backend event-model differences in the driver/adapter layer.
- **ADR-012**: keep completion submission APIs out of the trunk until their ownership and contract surface is ready.

If we optimize isolated hotspots before the scheduling shape is frozen, we risk performing the work twice:
first for readiness semantics, then again when the completion-style path becomes real.

## Decision
We will treat **driver scheduling** as the next trunk-level foundation step.

1. **Freeze the scheduling kernel before changing hot-path containers.**
   - The semantic reasons for driver-side work must become explicit and stable.
   - The current concrete queues are treated as an implementation detail, not the architecture.

2. **Keep user-visible semantics unchanged while we reshape internals.**
   - No new externally visible semantics.
   - P0 contracts remain the source of truth.

3. **Add internal observability before aggressive optimization.**
   - Measure scheduler enqueue volume, de-dup pressure, and follow-up work.
   - Measure RX materialization volume before redesigning the zero-copy path.

   BigStep-28 Step-4 lands the first batch of driver-kernel counters (enqueue volume, stale-skip volume, task submit/finish/reclaim pressure) as stable metric suffixes in `spark_uci::names::metrics`.

   BigStep-28 Step-5 then uses that evidence to reduce steady-state churn without changing semantics: interest synchronization becomes edge-triggered on desired-interest changes, `reactor.register()` is de-duplicated via per-slot cached state, and non-draining `WriteKick` becomes WRITE-edge driven instead of a per-tick scan.

4. **Defer data-structure replacement until the kernel shape is explicit.**
   - Per-channel flags / single-enqueue queues are a likely implementation target,
     but they follow the semantic freeze; they do not define it.
   - The first trunk step is to make the kernel vocabulary explicit in code (`ScheduleReason`)
     and to centralize bounded follow-up handling without changing semantics.
   - The second trunk step is to freeze single-enqueue semantics per `(chan_id, reason)` with
     generation-aware slot state before removing the remaining global `HashSet<u32>` hot-path checks.
   - The third trunk step is to move the one-task-per-channel contract itself into
     generation-aware slot-local task state, so task ownership and slot reuse rules no longer depend
     on global hash membership.

5. **Defer RX zero-copy broadening until ownership boundaries are clearer.**
   - The current copy-first lease materialization remains the safe trunk choice
     until scheduling and future submission lifetimes are better pinned down.

## Stable scheduling reasons (kernel vocabulary)
The driver-side work model should converge on explicit internal reasons such as:
- `InterestSync`
- `DrainingFlush`
- `WriteKick`
- `FlushFollowup`
- `ReadKick`
- `Reclaim`

The exact storage/layout may change, but these semantic buckets must remain visible and reviewable.

## Consequences
- The next iteration will prioritize **architecture-first** refactoring over isolated micro-bench wins.
- Driver internals become easier to map to readiness today and completion tomorrow.
- Future hot-path changes become cheaper to review because the semantic kernel is already frozen.
- RX zero-copy work remains on the roadmap, but only after the surrounding ownership model is less likely to churn.
