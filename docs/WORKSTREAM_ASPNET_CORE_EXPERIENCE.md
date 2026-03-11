# Workstream: ASP.NET Core 体验优化落地（Options 强默认 + 可解释配置）

## North Star 目标
- **Options 化装配**：80% 用户只需要少量字段即可上线（强默认、少参数、清晰错误）。
- **单一事实来源（SSOT）**：Profile/Options 是唯一用户入口，内部 normalize/validate 只存在一处。
- **可解释配置**：可输出 effective config（运行时真正生效的值），用于审计/排障。

## 现状与差距

### 1) Mgmt Profile v1 与 ServerConfig 已收口为单一事实来源
- `ServerConfig` 仅保存 `mgmt: MgmtTransportProfileV1` 作为管理面配置；
- 兼容 builder API（如 `with_max_head_bytes/with_max_headers/...`）内部全部委托到 `mgmt`；
- transport options/config/perf config 构建统一从 `mgmt` 派生。

**收益：**
- 默认值与 normalize 只在 profile 侧维护；
- hosting/ember/tests 使用同一份事实来源；
- 用户入口清晰，不再双轨写入。

### 2) 缺少 effective config 输出
- `DataPlaneOptions -> DataPlaneConfig` 有 normalize，但缺少稳定的 `describe_effective()` / `dump_effective()`。

## 最新进展（BigStep-30A.2）

- `spark-transport` 增加稳定结构化快照：`DataPlaneConfig::describe_effective()` 与
  `DataPlaneOptions::describe_effective()`，覆盖 bind/backlog/budget、watermark、flush policy、
  hard cap、framing 与 HTTP limits（当 framing 为 Http1 时）。
- `spark-host` 增加：
  - `MgmtTransportProfileV1::describe_effective_config()`（profile 级别有效值）；
  - `ServerConfig::describe_effective_config()`（同时给出 default 与 perf overlay 生效后的 transport 快照）。
- perf overlay 覆盖边界被显式冻结为结构化 contract：
  `DataPlaneConfig::perf_overlay_boundary()`。
  - 允许覆盖：`watermark`、`flush_policy`、`budget`、`emit_evidence_log`；
  - 绝不覆盖：`bind`、`max_accept_per_tick(backlog)`、`max_channels`、`framing`、`drain_timeout`、`max_pending_write_bytes`。

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

## 最新进展（T3）
- `ServerConfig` 新增管理面 timeout/overload 配置入口，保持 profile 作为单一事实来源。
- `HostBuilder` 默认诊断路由中，`/readyz` 不再只看 draining，而是组合 listener 与依赖健康。
- 路由层新增 request timeout 元数据，可在 route 与 group 维度覆盖。
- transport-backed 管理面服务已将 request timeout / overload / draining 真正接入执行路径，并对 route/group/default timeout 进行 504 语义闭环验证。

## T5/T6 增量（并发治理与扩展口子）
- 新增 app-service 级 Options：`max_inflight_per_connection`、`max_queue_per_connection`、`overload_action`，通过 `ChannelPipelineBuilder::app_service_options(...)` 接入；默认 profile 不变。
- 过载行为明确化：
  - `FailFast`：直接透传异常（拒绝当前请求）；
  - `Backpressure`：触发 `writabilityChanged(false)` 让上层 handler 可感知并限流；
  - `CloseConnection`：在恶性过载场景主动关闭连接。
- 该扩展保持主干热路径不变：仅在 `AppServiceHandler` 的入站排队分支执行，不引入额外动态分发层。

### 最小示例 1：metrics-style hook（writability/backpressure 观察）
```rust
struct MetricsHook;

impl<A, Ev, Io> spark_transport::async_bridge::pipeline::ChannelHandler<A, Ev, Io> for MetricsHook
where
    Ev: spark_transport::evidence::EvidenceSink,
    Io: spark_transport::async_bridge::DynChannel,
{
    fn channel_writability_changed(
        &mut self,
        ctx: &mut dyn spark_transport::async_bridge::pipeline::context::ChannelHandlerContext<A>,
        _state: &mut spark_transport::async_bridge::channel_state::ChannelState<Ev, Io>,
        is_writable: bool,
    ) -> spark_transport::Result<()> {
        // 这里可桥接到 metrics/tracing。
        let _ = is_writable;
        ctx.fire_channel_writability_changed(is_writable)
    }
}
```

### 最小示例 2：auth/logging-style middleware hook（request 生命周期）
```rust
struct LoggingHook;

impl<A, Ev, Io> spark_transport::async_bridge::pipeline::ChannelHandler<A, Ev, Io> for LoggingHook
where
    Ev: spark_transport::evidence::EvidenceSink,
    Io: spark_transport::async_bridge::DynChannel,
{
    fn channel_read(
        &mut self,
        ctx: &mut dyn spark_transport::async_bridge::pipeline::context::ChannelHandlerContext<A>,
        _state: &mut spark_transport::async_bridge::channel_state::ChannelState<Ev, Io>,
        msg: spark_buffer::Bytes,
    ) -> spark_transport::Result<()> {
        // 这里可做 auth/logging/审计。
        ctx.fire_channel_read(msg)
    }
}
```

## 最新进展（T3 深水区：drain/readiness 生命周期闭环）
- `EmberState` 扩展为轻量多信号模型：`accepting_new_requests`、`active_requests`、`overloaded`，并与 `draining/listener_ready/dependencies_ready` 联动。
- std server 与 transport server 都在请求进入/退出时维护 `active_requests`，在过载拒绝时更新 `overloaded`，并在 drain 后停止受理新请求。
- `/readyz` 语义收敛为“可接流量”判定（draining/listener/dependency/accepting/overload）；`/healthz` 继续只表达存活（drain 中保持 200）。
- in-flight 收敛边界明确：按 route/default request timeout deadline 收敛，不引入依赖探针框架。
