# ADR-007: Management Plane Profile v1 (Single Source of Truth)

## Status

Accepted (2026-03-02)

## Context

Spark 的 North Star 要求管理面（HTTP）作为“内部生态自举”闭环：

- mgmt plane 必须跑在 Spark transport core 之上（dogfooding）。
- mgmt plane 必须具有可预测的资源上限（工业/金融/军事生产要求）。
- mgmt 的 HTTP 限制（head/body/request）、并发隔离（in-flight cap）、transport 资源（channels/accept/budget）需要在多个层面一致：
  - `spark-host` 的装配层（HostBuilder / 默认 diagnostics）
  - `spark-ember` 的 mgmt adapter（std-only 与 transport-backed）
  - contract/smoke tests

若这些配置散落在多个地方（ServerConfig、TransportServer、tests），长期演进会出现：

- 口径漂移（不同模块解释不同）
- dogfooding 不可复现（不同入口生成不同 profile）
- 后续引入 IOCP/epoll/kqueue/WASI 时难以保持一致

## Decision

引入 `MgmtTransportProfileV1` 作为 mgmt plane 的单一事实来源（single source of truth），并要求：

1) Profile 明确分解为：
   - `MgmtHttpLimits`：HTTP request head/body/request 限制（含 `effective_max_request_bytes`）
   - `MgmtIsolationOptions`：in-flight cap + transport capacity (channels/accept) + poll budget

2) transport-backed mgmt server 只能通过 `MgmtTransportProfileV1` 生成 dataplane config：
   - `profile.transport_config()`
   - `profile.transport_perf_config()`

3) `ServerConfig` 保留为产品层配置入口，但 profile 的生成必须集中到：
   - `ServerConfig::mgmt_profile_v1()`
   - `ServerConfig::management_transport_*` 统一委托给 profile（避免重复实现）。

## Consequences

- 好处：
  - mgmt profile 的语义与默认值被稳定冻结，利于 dogfooding 与跨后端一致性。
  - hosting/ember/tests 共享同一 profile 生成逻辑，减少漂移与返工。
  - 更适配 Embedded/WASM 扩展：profile 是纯数据（runtime-neutral），后端只需适配 transport。

- 代价：
  - transport-backed mgmt server 构造函数签名变化（需要显式传入 profile）。
  - 少量调用方与 smoke tests 需要同步更新。
