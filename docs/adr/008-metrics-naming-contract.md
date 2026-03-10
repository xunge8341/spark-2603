# ADR-008: Metrics naming contract is anchored in `spark_uci::names::metrics`

- Status: Accepted
- Date: 2026-03-03

## Context

Spark 目标要求：
- observability 是一等公民，且 **跨 OS/CPU/后端**口径一致；
- mgmt-plane 必须 dogfooding dataplane 的观测口径；
- 生产环境（工业/金融/军事）里，指标命名漂移会导致监控告警/仪表板/回归阈值静默失效。

在此前实现中，Prometheus exporter 使用分散的字符串常量（如 `spark_dp_accepted_total`）。
这会引入以下风险：
- 字符串拼写错误无法被编译器捕获；
- 多后端/多发行物容易出现“同指标不同名”的分裂；
- mgmt-plane 与 dataplane 口径难以做硬门禁。

## Decision

1) 将 metrics naming contract 的单一事实来源冻结在 `spark_uci::names::metrics`。
2) Exporter（如 `spark-metrics-prometheus`）只能通过引用这些常量生成指标名；
   - exporter 可以添加稳定前缀（本仓库约定为 `spark_dp_`），但 suffix 必须来自常量。
3) 将派生指标（derived gauges）也纳入 naming contract（同样使用常量）。
4) dogfooding smoke tests 必须通过常量拼接期望指标名，从而把“口径漂移”转化为编译期/测试期失败。

## Consequences

- Pros:
  - 口径漂移变为编译期失败（最佳失败模式）；
  - mgmt-plane 与 dataplane 可观测性一致性更容易 contract 化；
  - 为 IOCP/epoll/kqueue/WASI/WASM/embedded 的多后端扩展提供稳定锚点。

- Cons:
  - Exporter 需要依赖 `spark-uci`（可接受：exporter 是 leaf crate）。

## Notes

- `spark-transport` 本身仍保持 runtime-neutral 与 exporter-free。
- `spark-uci` 为 no_std，可作为“协议/口径”稳定锚点。
