# Workstream: C++ 级性能的“可控分配”（BigStep-29/30 目标）

## North Star 对“可控分配”的可验收定义
- 热路径默认不产生不可控堆分配；
- 若存在分配，必须：
  1) 有明确预算（hard cap/upper bound），
  2) 有可观测计量（alloc/realloc/bytes）。

## 现状与风险点

### 1) OutboundBuffer 队列可被应用层无限驱动增长
- 当前 watermarks 只改变 writability（backpressure），不会拒绝继续 enqueue。
- `VecDeque<OutboundFrame>` 可能持续增长并触发 reallocate。

**风险：**
- “可控分配”无法证明；
- 在异常业务/恶意输入下可能变成内存放大器。

### 2) Cumulation tail（BytesMut）存在“增长到 max_frame”的 realloc
- tail 初始容量是 `min(max_frame, 64KiB)`，但在逼近 max_frame 的 framing 下仍会增长。

**结论：**
- 这是“预算内分配”，但需要证据：
  - realloc 次数/增长曲线；
  - 与 `max_frame` 的关系。

## 落地路线（保持默认语义兼容，逐步收紧）

### 阶段 A：先把“分配是否可控”变成可解释证据
**做法：**
- 为下列结构增加轻量计数（不引入 allocator hook）：
  - `OutboundBuffer`：记录 `VecDeque` capacity 变化次数与峰值。
  - `BytesMut`（cumulation tail）：记录 `capacity()` 增长次数与峰值。
- 输出到 perf_baseline 摘要（先脚本/ignored test，不做强门禁）。

### 阶段 B：为 outbound 引入 hard cap（可选项，默认不破坏现有行为）
**做法：**
- 在 `DataPlaneOptions`/Profile 增加：`max_pending_write_bytes`（hard cap）
  - 默认值：`usize::MAX` 或 “高水位 * K”（保持兼容）
- `OutboundBuffer::enqueue` 超过 hard cap：
  - 返回 `KernelError::ResourceExhausted`（或稳定 error code）
  - mgmt-plane 可以选择：直接 503/关闭连接（生产可控）。

### 阶段 C：只在证据表明必要时再做容器替换
候选：
- bounded ring buffer（一次性分配固定槽位）
- frame slot pooling（复用 OutboundFrame 节点）

**原则：**
- 先有证据（realloc/峰值/抖动），再决定是否值得引入复杂度。

## 关键落点（代码索引）
- outbound 队列：`crates/spark-transport/src/async_bridge/outbound_buffer.rs`
- watermark 语义：`crates/spark-transport/src/policy/*` + `ChannelState` writability
- cumulation tail：`crates/spark-buffer/src/cumulation.rs` + `crates/spark-buffer/src/bytes_mut.rs`
- perf baseline：`perf/*` + `scripts/perf_baseline.*`


## BigStep-29B 实施落地（默认兼容）
- `OutboundBuffer` 新增轻量证据：
  - `queue_capacity_growth_count`
  - `peak_queue_len`
  - `peak_pending_bytes`
- `BytesMut` / `Cumulation` 新增 tail 证据：
  - `capacity_growth_count` / `tail_capacity_growth_count`
  - `peak_capacity` / `tail_peak_capacity`
- 在 `DataPlaneOptions` / `DataPlaneConfig` 增加 `max_pending_write_bytes`，并统一走 normalize。
- hard cap 检查统一收敛到 `OutboundBuffer::enqueue`（单点 budget check）。
  - 默认值为 `usize::MAX`，保持现有行为兼容。
  - 超限返回稳定错误（当前映射为 `KernelError::NoMem`）。
- perf baseline 输出追加 alloc evidence 摘要字段，便于回归比对。

## T4：Perf gate 与 alloc 证据接线

- `perf_report.json` 的 global 段接入 alloc 相关证据：
  - `alloc_count`（当前映射 `ob_q_growth`）
  - `alloc_bytes`（预留；后续可接 feature-gated allocator 统计）
  - `alloc_bytes_unavailable_reason`（明确说明当前为何为 `null`，避免语义不清）
- gate 对 `peak_inflight_buffer_bytes` 建立阈值，确保 batching/flush 策略演进不会放大驻留内存上界。
- 该接线保持“证据先行”：在未启用统一 allocator 统计前，不伪造 alloc bytes 数字。
