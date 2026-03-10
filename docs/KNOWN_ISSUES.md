# Known Issues（登记册）

本文件用于登记已知问题，避免在主干迭代中反复“口头提起但无人负责”。

## KI-001: Windows mio write-pressure 前进性停滞（P0）

- 现象：
  - `cargo test -p spark-transport-mio --test write_pressure_smoke` 在 Windows 上持续失败：只接收少量 reply（常见为 2 个 256KiB 包）后停滞，15s 超时。
- 影响：
  - 属于跨后端“前进性”语义风险点；对工业/金融/军事场景的尾延迟与恢复能力有直接影响。
- 现状：
  - 已尝试在 driver 侧补充 bounded write-kick / read-kick，但问题在 Windows/mio 上仍可复现。
  - 为避免阻塞 Phase-1（Transport Core GA）主干，当前对 Windows 平台 **临时 ignore**，不作为 verify.ps1 的阻塞 gate。
- 计划（归属 Milestone 4/5）：
  1) 追加 Windows 专用的 reactor/interest tracing（可选 feature），捕获 READ/WRITE interest 变化与 poll 事件序列；
  2) 在 IOCP bring-up 时统一审计：edge/level/completion 的 event contract 与 driver 前进性策略；
  3) 将该 smoke 升格为 Windows/IOCP 的 P0 contract（必须过）。
