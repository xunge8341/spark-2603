# ADR-011: Ember transport-mgmt backend injection

## Status
Accepted — 2026-03-03

## Context
The management plane (HTTP/1) is a dogfooding gate: it must run on top of the same transport stack as the dataplane.
In earlier iterations, `spark-ember`'s transport-backed mgmt server (`TransportServer`) directly depended on the mio backend.

This violates the North Star layering:
- `spark-ember` should be runtime/backend neutral (like ASP.NET Core hosting).
- Backend crates (`spark-transport-mio`, future `spark-transport-iocp/epoll/kqueue/...`) must stay leaf-only.

Additionally, `spark-core::Service` uses native async in trait (not object-safe), so a dyn-based backend abstraction is not viable.

## Decision
1) Remove the direct dependency from `spark-ember` to any backend crate.
2) Make `TransportServer` spawn via an injected closure:
   - `TransportServer::try_spawn_with(spawn_fn)`
   - `TransportServer::try_spawn_perf_with(spawn_fn)`
3) Introduce `spark-dist-iocp` as a Windows-first distribution crate that wires host + mgmt + dataplane through the IOCP boundary.

## Rationale
- Keeps ember runtime-neutral and prevents semantic drift between mgmt and dataplane during multi-backend expansion.
- Preserves hot-path static dispatch by keeping backend spawn monomorphized.
- Enables internal ecosystem self-bootstrapping on Windows without blocking on the full native IOCP completion driver.

## Consequences
- Distribution crates must provide a mgmt spawn closure alongside the dataplane spawn closure.
- The mgmt server handle surface remains stable while backend implementations evolve.

## Follow-ups
- Add a contract gate that asserts mgmt and dataplane observability names are identical for each distribution.
- Bring up native IOCP completion backend behind the stable `spark-transport-iocp` boundary.
