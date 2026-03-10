# ADR-006: Dogfooding Gate for Management Plane (Transport-Backed) + Ephemeral Bind Support

## Status

Accepted (2026-03-02)

## Context

Spark 的 North Star 要求“内部生态自举”：控制面/管理面（HTTP）必须首先跑在同一套
transport core 之上，并复用 framing / evidence / metrics 的稳定口径。

在工程落地层面，dogfooding 必须可被 CI 持续验证；同时，CI/本地并行跑测试时不能依赖固定端口，
否则会出现端口冲突导致的偶发失败。

## Decision

1) 将 transport-backed mgmt-plane 作为 P0 dogfooding gate：
   - 在发行物（`spark-dist-mio`）中默认启用 `spark-ember` 的 `transport-mgmt` feature；
   - 增加一个跨平台 smoke test：启动 transport-backed mgmt server，发起 `GET /healthz`，
     必须返回 `200 OK`。

2) 为可靠的 dogfooding 测试与 embedding 场景引入“ephemeral bind 可连接”能力：
   - TCP dataplane spawn API 返回实际绑定地址（`local_addr`），支持 `bind=127.0.0.1:0`。
   - mgmt server spawn API 同样返回 `addr`，避免端口探测/抢占的竞态。

## Consequences

- 好处：
  - dogfooding 从“可选演示”升级为“可回归门禁”；
  - 测试与 embedding 场景端口分配稳定，CI 不再依赖全局固定端口；
  - 更贴近工业/金融/军事生产要求：可复现、可审计、可持续验证。

- 代价：
  - spawn API 返回类型调整（从 `JoinHandle` 到 `Handle{join, local_addr}`）。
  - 需要更新少量调用方与测试。
