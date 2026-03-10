# Workstream: ASP.NET Core 体验优化落地（Options 强默认 + 可解释配置）

## North Star 目标
- **Options 化装配**：80% 用户只需要少量字段即可上线（强默认、少参数、清晰错误）。
- **单一事实来源（SSOT）**：Profile/Options 是唯一用户入口，内部 normalize/validate 只存在一处。
- **可解释配置**：可输出 effective config（运行时真正生效的值），用于审计/排障。

## 现状与差距

### 1) Mgmt Profile v1 已冻结，但 ServerConfig 仍平铺字段再组装 profile
- `docs/MGMT_PROFILE_V1.md` 声明 profile 是 single source of truth；
- 但 `crates/spark-host/src/config.rs::ServerConfig` 仍保存一套 mgmt 字段，然后 `mgmt_profile_v1()` 组装。

**风险：**
- 默认值漂移（两个地方都在“定义默认”）；
- normalize/validate 逻辑分散；
- 用户不清楚“改哪里才生效”。

### 2) 缺少 effective config 输出
- `DataPlaneOptions -> DataPlaneConfig` 有 normalize，但缺少稳定的 `describe_effective()` / `dump_effective()`。

## 落地路线（尽量低风险，先做不改变 dataplane 语义的整理）

### 阶段 A：ServerConfig 收敛为 profile 驱动
**做法：**
- `ServerConfig` 只持有 `mgmt: MgmtTransportProfileV1`（以及 name/shutdown 等少量宿主字段）。
- 现有 `with_max_head_bytes/with_max_headers/...` 这类 builder API：
  - 保留，但内部直接委托修改 `mgmt.http.*`。
- 所有 transport options/config/perf overlay 的构建都从 `mgmt` 走：
  - `mgmt.transport_options()`
  - `mgmt.transport_config()`
  - `mgmt.transport_perf_config()`

**收益：**
- profile 成为真正 SSOT；
- hosting/dogfooding/tests 统一；
- 文档与代码一致。

### 阶段 B：增加 effective config 输出（审计/排障）
**做法：**
- 在 `spark-transport` 增加：
  - `DataPlaneConfig::describe_effective()` → 返回稳定 struct（不直接拼字符串）
  - `impl Display` / `to_lines()` 用于日志输出
- 在 `spark-host` 的启动日志与 `/healthz` 里可选输出（受开关控制）。

### 阶段 C：冻结默认集合（Default / Perf）
**做法：**
- 明确 `with_perf_defaults()` 的覆盖面与不变量（哪些字段绝不被 perf overlay 改动）。
- 在 `docs/MGMT_PROFILE_V1.md` 与 `docs/OBSERVABILITY_CONTRACT.md` 中同步。

## 关键落点（代码索引）
- ServerConfig：`crates/spark-host/src/config.rs`
- Mgmt Profile：`crates/spark-host/src/mgmt_profile.rs` + `docs/MGMT_PROFILE_V1.md`
- Options/Config：`crates/spark-transport/src/config.rs`（或相关模块）
- Hosting & dogfooding：`crates/spark-ember` / `crates/spark-host`
