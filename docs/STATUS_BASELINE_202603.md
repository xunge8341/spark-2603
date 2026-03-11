# STATUS BASELINE 2026-03 (T0 冻结)

目的：冻结“代码状态 / 脚本规则 / 文档表述”同一基线，后续修复必须以此为准。

## 1) 当前支持矩阵

| 维度 | 状态 | 证据 |
|---|---|---|
| 传输主干（spark-transport + mio） | 已支持（主路径） | `verify.sh` 默认阻断项 + contract suite |
| Windows 分发路径（spark-dist-iocp） | 部分支持（phase-0） | iocp crate 默认 wrapper 到 mio；native completion feature 可选 |
| IOCP native completion | 未完成（原型可测） | 仅在 `SPARK_VERIFY_COMPLETION_GATE=1` 下执行 |
| RX zero-copy（stream） | 部分支持（Phase-A 边界） | stream 可借用 token；仍有后续 copy 路径 |
| RX zero-copy（datagram） | 未完成（保守 owned） | datagram 保持 owned materialize |
| perf/bench regression gate | 部分支持（脚本存在） | 默认 verify 不阻断，需显式环境变量启用 |

## 2) 当前风险矩阵

| 风险ID | 风险描述 | 级别 | 当前阻断策略 |
|---|---|---|---|
| R-001 | Windows mio write-pressure 前进性停滞 | P0 | 已知问题登记；Windows 用例当前 ignore，默认 verify 不阻断 |
| R-002 | IOCP native completion 非默认语义路径 | P1 | completion gate 为 opt-in，不是默认阻断 |
| R-003 | perf/bench 回归仅在显式开启时发现 | P1 | `SPARK_VERIFY_PERF_GATE` / `SPARK_VERIFY_BENCH_GATE` 控制 |
| R-004 | unsafe 边界与脚本白名单漂移 | P0 | 已通过本轮修正，脚本与源码位置对齐 |

## 3) unsafe 扫描与治理脚本状态（当前）

### 当前真实状态
- `crates/spark-transport/src/async_bridge/channel_state.rs` 已移除 `unsafe`。
- `docs/UNSAFE_REGISTRY.md` 是 unsafe 台账的单一事实源（single source of truth）。
- `scripts/unsafe_audit.sh` 与 `scripts/unsafe_audit.ps1` 均为 registry-synced：
  - 扫描 `crates/**/*.rs` 的 `unsafe` 命中；
  - 强制 `unsafe` 前具备 `// SAFETY:` 注释；
  - 校验 registry 缺失条目与失效条目。

### 当前含 unsafe 的文件类别
- transport 主干：`crates/spark-transport/src/lease.rs`、`crates/spark-transport/src/reactor/event_buf.rs`
- iocp native-completion：`crates/spark-transport-iocp/src/native_completion.rs`
- contract tests / helper tests：
  - `crates/spark-transport-contract/tests/framing_roundtrip.rs`
  - `crates/spark-transport-contract/tests/framing_roundtrip_fuzz.rs`
  - `crates/spark-transport-contract/tests/rx_lease_observability.rs`
  - `crates/spark-transport-iocp/tests/native_completion_smoke.rs`
  - `crates/spark-buffer/src/bytes.rs`（测试模块内）
- `spark-buffer/src/scan.rs`

## 4) verify gate 阻断策略（当前）

### 默认阻断（失败即失败）
1. fmt
2. clippy
3. deps invariants
4. panic-free scan
5. unsafe audit
6. workspace compile (`--no-run`)
7. contract suite
8. workspace tests（排除 contract suite 重复）

### 默认非阻断（需显式开启）
- completion gate（`SPARK_VERIFY_COMPLETION_GATE=1`）
- perf gate（`SPARK_VERIFY_PERF_GATE=1`）
- bench gate（`SPARK_VERIFY_BENCH_GATE=1`）

## 5) 本轮修复清单

### P0
- 对齐 unsafe 审计白名单与实际源码位置，消除“脚本允许清单 != 源码事实”。
- 文档统一承认 Windows write-pressure 与 IOCP native completion 当前状态，不再隐含“已完整支持”。

### P1
- 把 completion/perf/bench gate 的“可选且非默认阻断”状态在文档中明确写死。
- 后续分平台提升 gate 策略（nightly/default）时，同步更新本基线文档。

## 6) 禁止事项（冻结）

- 禁止把“部分支持/未完成”写成模糊措辞（例如 experimental-ish but production-minded）。
- 禁止通过删除文档条目来规避状态不一致。
- 禁止在未升级 gate 策略前把相关能力写成“默认阻断已覆盖”。
