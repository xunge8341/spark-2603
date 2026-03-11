# P0 Gates（主干门禁集合）

P0 的定义：**任何后端/平台在引入/重构后都必须通过**，否则视为语义回退，不允许合入。

本仓库的 P0 门禁由三部分构成（缺一不可）：

> 工程硬标准：所有 verify/perf/bench gates 使用 `--locked`，确保构建可重复且不会隐式改写 lockfile。

1. **Contract（语义一致性）**
2. **Baseline（性能/资源基线）**
3. **Dogfooding（内部生态自举）**

## 1) Contract（P0）

由 `crates/spark-transport-contract` 承载。

### P0 Suite（所有后端必过）
- `tests/contract_suite.rs`
  - Backpressure enter/exit（PauseRead/ResumeRead + Interest 合规）
  - Draining close-after-flush / timeout（证据事件 + close 语义）
  - Interest minimal contract（避免 busy-loop）
  - CloseRequested / PeerHalfClose evidence（稳定口径）
  - Close idempotent + close-ish error

### P0 单项（按能力/平台）
- `tests/close_evidence.rs`：CloseRequested + CloseComplete 证据事件（driver 级）
- `tests/abortive_close_evidence.rs`：Reset/AbortiveClose 证据事件（driver 级）
- `tests/framing_roundtrip.rs` / `tests/framing_roundtrip_fuzz.rs`：内置 framing（Line/Delimiter/LengthField/Varint32）收发对称 + 随机切分 segments 回归（Milestone-2 主干门禁）
- `tests/tcp_half_close.rs`：TCP half-close 不应中断 pending writes（适用于 stream 后端）
- `tests/*_error_classification.rs`：accept/connect/tcp/udp 错误分类稳定映射
- `tests/write_pressure_progress.rs`：写压力下公平性预算与前进性（driver 级）

## 2) Baseline（P0）

- `scripts/perf_gate.(sh|ps1)`：吞吐/系统调用/拷贝压力的粗粒度回归门禁
- `scripts/bench_gate.(sh|ps1)`：TCP echo 基准门禁

> Baseline 默认走平台基线文件 `perf/baselines/*`，并允许在 CI/Nightly 上设置阈值回归报警。

## 3) Dogfooding（P0）

- 管理面（HTTP/1）必须运行在 transport 上（`spark-ember` / `spark-host`），并复用同一套观测口径。
- Evidence/metrics 名称必须来自 `spark_uci::names`（见 `docs/OBSERVABILITY_CONTRACT.md`）。
- `/metrics` 输出必须包含 `spark_dp_<name>`（name 来自 `spark_uci::names::metrics`），由 `crates/spark-dist-mio/tests/smoke_mgmt_metrics.rs` 断言。
- 管理面 profile 必须通过 `MgmtTransportProfileV1` 生成（single source of truth，见 `docs/MGMT_PROFILE_V1.md`）。
- Windows（IOCP 分发路径）必须具备可运行的 dataplane smoke：`crates/spark-dist-iocp/tests/dataplane_echo_smoke.rs`。


## Optional nightly gates

- Unsafe audit: `scripts/unsafe_audit.(sh|ps1)` enforces `// SAFETY:` documentation + `docs/UNSAFE_REGISTRY.md` bidirectional sync (single source of truth).
- Completion prototype (Windows): set `SPARK_VERIFY_COMPLETION_GATE=1` to run the native IOCP completion smoke test.
- no_std/alloc target gate: `scripts/nostd_gate.(sh|ps1)` ensures L0/L1 crates compile for embedded + WASM targets.
