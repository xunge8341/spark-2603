# Gap Status (Current vs North Star)

This document is the trunk baseline. It must match code and verification scripts.

> Baseline snapshot: 2026-03 (T2 Windows/IOCP status honesty + forward-progress gate).

## Achieved (trunk)

### Driver scheduling kernel is explicit and reviewable
- `ScheduleReason` is frozen: `InterestSync / DrainingFlush / WriteKick / FlushFollowup / ReadKick / Reclaim`.
- Single-enqueue semantics and generation-aware stale-drop are in place.
- `install_channel() -> sync_interest(chan_id)` contract baseline is frozen.

### Safety/quality gates are executable
- `scripts/verify.sh` / `scripts/verify.ps1` blocks on: fmt, clippy, deps invariants, panic scan, unsafe audit（registry-synced）, workspace compile, contract suite, workspace tests.
- perf/bench/native-completion are optional explicit gates (`SPARK_VERIFY_*`).
- verify output now prints Linux status / Windows status / known gaps explicitly.

### Windows backend status is now explicit and unified
- unsafe inventory is unified by `docs/UNSAFE_REGISTRY.md` (single source of truth), with shell + PowerShell audits enforcing documented `SAFETY` and registry bidirectional consistency.
- `spark-transport-iocp` is classified as a **phase-0 compatibility layer**, not a native IOCP dataplane.
- Default dataplane is wrapper mode (delegates readiness engine) and remains **not production-ready as native IOCP**.
- Native completion remains an opt-in prototype (`--features native-completion`) for phase-x validation.

## Remaining gaps (priority order)

### Gap 1 — RX zero-copy not fully closed-loop
**Now:** stream can borrow token-backed bytes during dispatch, but framing/cumulation still has copy paths; datagram remains owned.
**North Star:** stream RX zero-copy-first end-to-end, copy only for explicit retention/fallback with metrics.
**Next:** evaluate Phase-B only with evidence (owned-segment append or equivalent), keep ownership boundary auditable.

### Gap 2 — Multi-backend parity
**Now:** mio is production dataplane baseline; IOCP is phase-0 compatibility layer + partial native-completion prototype.
**North Star:** IOCP/epoll/kqueue/io_uring pass same P0 contracts.

### Gap 3 — CI/nightly hard gates
**Now:** day-time `verify.*` keeps perf/bench/completion opt-in, while `verify_nightly.*` enables perf/bench/completion/no_std by default.
**North Star:** policy-driven default gates per platform with clear blocking rules.

### Gap 4 — Safety hardening depth
**Now:** panic-free scan + unsafe confinement + SAFETY comment checks are in place.
**North Star:** add fuzz/schedule-perturbation/loom/miri-sanitizer layers.

## Known blocking risk
- Windows mio `write_pressure_smoke` forward-progress stall remains open and is tracked in `docs/KNOWN_ISSUES.md`.
- This issue is now surfaced as an explicit known-failing gate/report item (no silent ignore policy).

## Update T3（超时/就绪/排水治理）
- 已在 host/ember 管理面补齐连接级 timeout（idle/read/write/headers）与请求级 timeout（默认 + route/group 覆盖）。
- `/healthz` 与 `/readyz` 语义分离：`/readyz` 现在同时检查 draining、listener readiness、dependency readiness。
- 增加过载治理配置：并发上限、每连接 inflight、队列上限与 reject policy（503/429/直接关闭）。
- transport-backed 管理面主路径已接入上述治理：draining 拒绝新请求、route/group/default request timeout 返回 504、全局并发与队列过载按 reject policy 生效。

## Update T4（性能证据与 gate 补齐）
- perf gate 从单点 `SPARK_PERF` 解析升级为“场景矩阵 + 基线对比”：`scripts/perf_gate.sh` 调用 `scripts/perf_report.sh` 生成 JSON/CSV，再按 `perf/baselines/perf_gate_*.json` 对比。
- 指标口径覆盖吞吐 + 尾延迟 + syscall/batching + copy + backpressure，并补充 `peak_rss_kib` 与 `peak_inflight_buffer_bytes`。
- 基线按 Unix/Windows 分离，显式避免“Linux 最优数字外推所有平台”。
- PowerShell perf gate 已与 shell 口径统一：同样走 `perf_report` JSON/CSV 合同，并按 scenario/global 双层阈值阻断。
- nightly 路径（`verify_nightly.sh/.ps1`）默认开启 perf gate，普通 verify 继续保持可选。
- perf gate 脚本完成“单核心收口”：新增 `scripts/perf_gate_core.py` 承担 baseline 解析与 JSON 对比，`perf_gate.sh/.ps1` 仅保留薄包装与依赖预检查，避免 sh/ps1 逻辑分叉。
- Windows/PowerShell 路径显式声明并预检查依赖：`bash`（用于 `perf_report.sh`）与 Python 3（`python3`/`py -3`/`python`），缺失时给出可操作错误提示。
- `benchmark/README.md` 补齐依赖矩阵与 verify/nightly 职责边界说明，明确普通 verify 保持 opt-in、nightly 默认阻断的原因。

## Update T5（主干并发与过载治理）
- `AppServiceHandler` 从“单 inflight + 无上界 queue”升级为可配置模型：`max_inflight_per_connection` + `max_queue_per_connection` + `overload_action`（FailFast / Backpressure / CloseConnection）。
- 默认值明确：`max_inflight_per_connection=1`、`max_queue_per_connection=1024`、`overload_action=FailFast`，确保不出现无限队列。
- 数据面指标已闭环 overload 可观测：新增 `overload_reject_total` / `overload_backpressure_total` / `overload_close_total` 与 `app_queue_high_watermark`，并在 `/metrics` 导出。
- 新增单测覆盖：队列上限、背压信号传播、拒绝关闭语义。

## Update T6（扩展点整理）
- pipeline builder 暴露 `app_service_options(...)` 作为 per-handler/per-protocol 覆盖入口（不引入额外多层 trait）。
- dataplane 配置新增并文档化 app-service 并发/队列/过载策略字段，并通过 `describe_effective()` 对外可审计。
- pipeline 对外 re-export 收敛到最小稳定入口（builder/handler/options/profile），并明确 internal module 非稳定承诺。
- 新增 `docs/EXTENSION_SURFACE.md`，固定连接/请求/metrics-tracing/codec-framing 的扩展边界与稳定承诺。
- `spark-dist-mio` 新增最小示例：`mgmt_metrics_hook.rs` 与 `mgmt_timeout_group.rs`，分别覆盖 diagnostics hook 与 group timeout 用法。


## Update T7（drain/readiness 生命周期闭环）
- `EmberState` 新增轻量状态面：`accepting_new_requests`、`active_requests`、`overloaded`，并保持 `draining/listener_ready/dependencies_ready` 组合可观测。
- `/readyz` 语义收敛为：非 draining、可接收新请求、listener ready、dependencies ready、非 overloaded。
- `/drain` 不再只是切标志：std/transport server 均在 drain 后拒绝新请求，并对在途请求按 request timeout deadline 收敛。
- 边界说明：当前依赖就绪仍是布尔聚合，不引入外部依赖探针框架。

## Update T7.1（transport-backed 生命周期回落补齐）
- transport-backed 管理面在 `try_spawn_with(_perf)` 成功后，保持与 std server 一致的启动状态：`listener_ready=true`、`accepting_new_requests=true`、`overloaded=false`。
- 新增 transport handle join wrapper：当 transport dataplane 线程退出（正常/panic）时，统一回落 `listener_ready=false`、`accepting_new_requests=false`、`overloaded=false`，避免“只在启动置 true，无退出回落”的状态漂移。
- `active_requests` 继续由请求级 guard 维护；单测补齐“请求结束回到 0、重复 decrement 不下溢”约束，确保 drain/退出期间不出现负数语义或泄漏。

## Update T7.2（transport-backed dataplane 端到端烟测补齐）
- 在 `crates/spark-dist-mio/tests/mgmt_dogfooding_smoke.rs` 新增 transport-backed E2E 覆盖，补齐此前仅 service-level 单测的缺口。
- 新增 route-level timeout 与 group-level default timeout 的真实 socket 验证，确保请求经 dataplane 后仍返回 504。
- 新增 overload 压力路径验证（受限并发/队列 + reject policy），在真实 transport 路径断言 429/503 语义。
- 新增 drain + inflight 收敛验证：先进入在途，再触发 `/drain`，新请求被拒绝且在途请求按 timeout deadline 收敛。
- 文档上明确“service-level 覆盖用于逻辑快速回归；E2E 覆盖用于 transport/dataplane 集成语义验收”。


## Update T7.3（小型硬化：examples compile gate + 扩展面一致性）
- `scripts/verify.sh` 与 `scripts/verify.ps1` 新增 `cargo test --workspace --examples --no-run --locked`，将最小示例纳入默认编译门禁，防止示例随主干 API 漂移后静默腐烂。
- 对扩展面文档与公开 re-export 做对齐复核：
  - transport pipeline 继续以 `ChannelPipelineBuilder` / `ChannelHandler` / `AppServiceOptions` / `OverloadAction` 为稳定入口；
  - host 层继续以 `HostBuilder` / `ServerConfig` / `MgmtGroup` / `EndpointBuilder` 为稳定入口。
- `transport_server.rs` 清理仅用于结构体占位、未参与运行路径的冗余字段，避免后续维护误判为“有效治理开关”。

## Update T7.4（仓库级执行约束固化）
- 新增仓库级 `AGENTS.md`，将 Codex 项目执行边界与工程红线收敛为单一事实来源。
- 明确跨安全边界业务交换网关约束：SIP JSON body、RTP 定长主头+TLV、`api_code` 受控 HTTP 路由模板执行、签名算法升级位预留。
- 固化 Rust 工程约束：panic-free、`unsafe` 受控边界、`--locked` 与 lockfile 一致性、最小公开 API 与指标命名集中化。
- `docs/NEW_SESSION_START_HERE.md` 已加入入口提示，要求新任务先对齐仓库级 `AGENTS.md`。
