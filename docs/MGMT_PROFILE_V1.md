# Mgmt Profile v1（管理面 Profile）

本文件冻结 **管理面（HTTP）Profile v1** 的配置语义与默认值。

目标：

- 管理面作为内部生态自举（dogfooding）闭环：默认跑在 Spark transport core 之上。
- 管理面必须具备可预测的资源上限（工业/金融/军事生产要求）。
- hosting / ember / tests 统一使用同一份 profile（single source of truth）。

## 核心结构

Profile 类型：`spark_host::MgmtTransportProfileV1`。

### 1) HTTP 限制（`MgmtHttpLimits`）

- `max_head_bytes`：request line + headers（含 CRLF）上限
- `max_headers`：header 数量上限
- `max_body_bytes`：body bytes 上限（Content-Length cap）
- `max_request_bytes`：请求总大小上限（head + body）

派生规则：

- `effective_max_request_bytes = max(max_request_bytes, max_head_bytes + max_body_bytes, 1)`
- transport framing 使用 `effective_max_request_bytes` 作为上限，确保 framing 与解码限制一致。

### 2) 隔离策略（`MgmtIsolationOptions`）

- `max_inflight`：并发请求上限（控制面隔离）
- `limits.max_channels`：mgmt transport 最大连接数
- `limits.max_accept_per_tick`：每 tick accept 上限
- `budget`：poll/driver tick 预算（events + nanos）

目的：

- mgmt 不是吞吐数据面，必须 bounded，避免拖垮 dataplane。
- 后续引入 IOCP/epoll/kqueue/WASI 时，保持一致的资源预算语义。

## Profile 到 Transport Config 的映射

- `profile.transport_options()` 生成 `spark_transport::DataPlaneOptions`
- `profile.transport_config()` 生成 `spark_transport::DataPlaneConfig`
- `profile.transport_perf_config()` 在保持 bind/framing/limits 的前提下叠加 perf overlay（用于压测/回归）

## 入口

- `spark_host::ServerConfig { mgmt: MgmtTransportProfileV1, .. }`
  - `ServerConfig` 内部仅保存 `mgmt` 这一份管理面事实。
  - `with_max_head_bytes/with_max_headers/...` 等兼容 builder API 仅委托修改 `mgmt`。
- `spark_host::ServerConfig::mgmt_profile_v1()`
  - 返回 `mgmt` 的克隆视图，保留兼容入口。
- `spark_ember::TransportServer`（transport-backed mgmt）
  - 只能通过 `MgmtTransportProfileV1` 启动，避免配置散落。
