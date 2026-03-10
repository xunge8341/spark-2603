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
