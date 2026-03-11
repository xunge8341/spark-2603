# Known Issues（登记册）

本文件登记“已知且被承认”的问题，并标注 gate 策略，避免口头状态漂移。

## KI-001: Windows mio write-pressure 前进性停滞（P0）

- 现象：
  - `cargo test -p spark-transport-mio --test write_pressure_smoke` 在 Windows 上可复现停滞与超时。
  - 测试已在源码中按平台标记 `#[cfg_attr(windows, ignore = ...)]`。
- 影响：
  - 跨后端前进性风险；影响尾延迟与恢复语义可信度。
- gate 现状：
  - 当前 `scripts/verify.sh` / `scripts/verify.ps1` 都不会阻断该 Windows 专项用例（因为该用例在 Windows 被 ignore）。
- 处置计划（P0/P1 分层）：
  1) P0：补齐 Windows reactor/interest tracing 证据链；
  2) P0：完成 root-cause 与修复，并取消该 ignore；
  3) P1：把该 smoke 纳入 Windows 默认阻断 gate。

## KI-002: IOCP native completion 仍非默认路径（P1）

- 现象：
  - `spark-transport-iocp` 默认 dataplane 仍是 phase-0 wrapper；native completion 仅在 `--features native-completion` 下测试。
- 影响：
  - IOCP 语义仍处于“原型验证 + 分发边界冻结”阶段，非默认生产路径。
- gate 现状：
  - `SPARK_VERIFY_COMPLETION_GATE=1` 时执行 completion tests；默认 verify 不阻断。
- 处置计划：
  1) P1：补齐 socket overlapped submit（AcceptEx/WSARecv/WSASend）契约测试；
  2) P1：提升为 Windows/nightly 默认 gate；
  3) P0（后续里程碑）：满足 contract 后再评估默认切换。

## Gate policy snapshot (2026-03)

- 默认阻断（`verify.sh`）：fmt / clippy / deps invariants / panic scan / unsafe audit / compile gate / contract suite / workspace tests。
- 默认非阻断（需显式开启）：
  - perf gate: `SPARK_VERIFY_PERF_GATE=1`
  - bench gate: `SPARK_VERIFY_BENCH_GATE=1`
  - completion gate: `SPARK_VERIFY_COMPLETION_GATE=1`
