# Extension Surface (T6)

本文档定义当前可依赖的最小扩展面：只整理稳定入口，不引入新的 middleware/plugin 框架。

## 1) 连接生命周期 hook

位置：`spark-transport::async_bridge::pipeline::ChannelHandler`。

可用事件（示意）：
- `handler_added` / `handler_removed`
- `channel_active` / `channel_inactive`
- `channel_writability_changed`
- `exception_caught`

约束：
- 扩展建议通过 `ChannelPipelineBuilder` 组装到既有 pipeline；
- 连接状态与驱动调度 contract 仍由 transport 内核维护，不在扩展点重写调度语义。

## 2) 请求生命周期 hook

位置：
- 数据面：`ChannelHandler::channel_read(...)` 可做日志/鉴权/审计，然后 `ctx.fire_channel_read(...)` 透传；
- 管理面：`spark-host` 的 `MgmtApp` / `MgmtGroup` / `EndpointBuilder`。

推荐模式：
- route/group 级 timeout 通过 `EndpointBuilder::with_request_timeout(...)` 与
  `MgmtGroup::with_request_timeout(...)` 声明；
- 不引入新的“通用插件 trait 层”，保持主干调用链可读。

## 3) metrics/tracing hook

位置：
- 默认诊断：`HostBuilder::use_default_diagnostics()` 提供 `/healthz` `/readyz` `/metrics` `/drain`；
- route 级指标：`RouteMetrics`（在 `HostBuilder::map_*` 包装中记录）；
- 连接/背压观测：`ChannelHandler::channel_writability_changed(...)`。

建议：
- 指标命名由集中模块输出（例如 `RouteMetrics` 与 dataplane metrics），
  业务 handler 避免散落硬编码指标名。

## 4) codec/framing 扩展位置

位置：`ChannelPipelineBuilder` 的 framing 相关 builder 选项：
- `LineOptions` / `DelimiterOptions`
- `LengthFieldOptions` / `Varint32Options`
- `Http1Options`（feature gate: `mgmt-http1`）

说明：
- framing 是 pipeline 组装阶段决定的输入边界，业务 handler 不应重新实现帧边界协议。

## 5) 稳定 API（当前承诺）

- `spark-host`：`HostBuilder`、`ServerConfig`、`MgmtGroup`、`EndpointBuilder`。
- `spark-transport::async_bridge::pipeline`：
  `ChannelPipelineBuilder`、`ChannelPipeline`、`ChannelHandler`、`AppServiceOptions`、`OverloadAction`、
  `FrameDecoderProfile`、`DelimiterSpec`。

这些入口用于“可持续扩展”；语义变化会通过文档和决策日志显式记录。

## 6) internal API（不承诺稳定）

以下模块当前属于实现细节，可能随重构调整：
- pipeline internal：`head` / `tail` / `context` / `event`（以及其他内部实现模块路径）
- 具体 runtime/driver glue 层。

外部调用方应通过稳定 re-export 访问，不直接依赖 internal module path。
