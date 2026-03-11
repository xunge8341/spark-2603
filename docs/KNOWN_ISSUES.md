# Known Issues（登记册）

本文件登记“已知且被承认”的问题，并标注 gate 策略，避免口头状态漂移。

## KI-001: Windows mio write-pressure 前进性停滞（P0, known-failing gate）

- 现象：
  - `cargo test -p spark-transport-mio --test write_pressure_smoke` 在 Windows 上可复现停滞与超时。
  - 测试已在源码中按平台标记 `#[cfg_attr(windows, ignore = ...)]`。
- 影响：
  - 跨后端前进性风险；影响尾延迟与恢复语义可信度。
- gate 现状（T2）：
  - `scripts/verify.sh` 明确打印该项为 **known-failing**，不再静默。
  - Windows 环境下，verify 会尝试执行 ignored 用例并记录“仍失败/已恢复”的状态提示。
  - 当前阶段仍不作为默认阻断（防止在 root-cause 未收敛前制造假绿）。
- 处置计划（P0/P1 分层）：
  1) P0：补齐 Windows reactor/interest tracing 证据链；
  2) P0：完成 root-cause 与修复，并取消该 ignore；
  3) P1：把该 smoke 纳入 Windows 默认阻断 gate。

## KI-002: IOCP 仍为 phase-0 compatibility layer（P1）

- 现象：
  - `spark-transport-iocp` 默认 dataplane 仍是 compatibility-layer wrapper，native completion 仅在 `--features native-completion` 下验证。
- 影响：
  - IOCP 语义仍处于“边界冻结 + 原型验证”阶段，**not production ready**（针对 native IOCP dataplane）。
- gate 现状：
  - Windows 专项测试集已覆盖：前进性、半关闭行为、背压下 drain、reactor 唤醒一致性。
  - 非 Windows 环境会显式 skip 并输出原因。
  - `SPARK_VERIFY_COMPLETION_GATE=1` 时执行 completion tests；默认 verify 不阻断。
- 处置计划：
  1) P1：补齐 socket overlapped submit（AcceptEx/WSARecv/WSASend）契约测试；
  2) P1：提升为 Windows/nightly 默认 gate；
  3) P0（后续里程碑）：满足 contract 后再评估默认切换。

## Gate policy snapshot (2026-03)

- 默认阻断（`verify.sh`）：fmt / clippy / deps invariants / panic scan / unsafe audit / compile gate / contract suite / workspace tests。
- 默认非阻断（需显式开启）
  - perf gate: `SPARK_VERIFY_PERF_GATE=1`
  - bench gate: `SPARK_VERIFY_BENCH_GATE=1`
  - completion gate: `SPARK_VERIFY_COMPLETION_GATE=1`
- 默认报告（known-failing 可见）
  - Windows `write_pressure_smoke` known-failing 状态输出（不允许静默 ignore）。


## KI-003: drain 收敛边界仍是“按请求 timeout 上界”（P1）

- 现象：
  - 当前 in-flight 收敛以路由 request timeout/default timeout 作为 deadline 上界。
  - 对“连接级半关闭 + 分阶段取消（headers/body/handler）”尚未做更细分的中断语义。
- 影响：
  - drain 语义已闭环，但长尾请求的停止粒度仍受超时配置精度限制。
- 当前策略：
  - 拒绝新请求 + 在途请求 deadline 收敛，保证可预期上界。
- 后续方向：
  - 若需要更强确定性，可在后续迭代引入分阶段 cancel token（不在本轮范围）。