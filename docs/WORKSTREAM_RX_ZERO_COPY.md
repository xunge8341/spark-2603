# Workstream: RX 零拷贝闭环（BigStep-29 目标）

## 背景与现状

当前 dataplane 的 stream RX 路径存在 **双重拷贝**：

1) lease/token read：`ReadData::Token(rx)` → `materialize_rx_token()` 通过 `rx_ptr_len` + `copy_from_parts` 生成 `Bytes`（一次 copy）
2) framing decode：`InboundState::append_stream_bytes()` 把 `bytes: &[u8]` 追加进 `Cumulation::tail(BytesMut)`（第二次 copy）

这会导致：
- lease path 反而比 copy path 更贵；
- `copied_bytes` 指标只覆盖 decode 阶段的 coalesce，而不包含 token materialize 的 copy。

## North Star 目标
- **stream RX zero-copy first**：能直接在 borrowed view 上 decode 的，绝不 materialize。
- copy 只能作为显式 fallback（跨调用保留/跨段拼接），并且 **copy/byte 可计量**。
- unsafe 继续 **confined**：不得把生命周期复杂度扩散到 pipeline 之外。

## 建议落地路线（分阶段、每阶段都保持 P0 contract 全绿）

### 阶段 A：先消灭“双重拷贝”（从 2 次降到 1 次）
**思路：**
- 对 `ReadData::Token(rx)`，不再立刻 materialize 成 `Bytes`。
- 改为在 `Channel::on_readable()` 内创建一个 RAII lease guard：
  - 通过 `rx_ptr_len(rx)` 生成 `&[u8]`（仅本次调用有效）
  - 调用 `fire_channel_read_raw_stream_slice(&[u8])` 走现有 fast-path
  - guard drop 时 `release_rx(rx)`

**收益：**
- token read 不再额外 copy；
- stream cumulation 仍有一次 copy（`Cumulation::push_bytes`），但总成本立刻减半。

**门禁：**
- contract suite 必须全绿（尤其 driver ordering / close semantics / framing roundtrip）
- 新增一个最小 contract：decode error/early return 也必须 release token exactly once（结构化 drop）

### 阶段 B：引入 “owned segment append” 关闭零拷贝闭环（从 1 次降到 0 次）
**思路：**
- 扩展 cumulation：新增 `Cumulation::push_segment(Bytes)`，用于把 **已拥有的 immutable segment** 直接入队（零拷贝）
- InboundState 增加 `append_stream_segment(Bytes)`：
  - 小 segment（< threshold）仍 copy into tail（减少碎片）
  - 大 segment 直接 push_segment（避免 copy）
- pipeline 增加 internal entrypoint（仅在无 pre-frame handlers 时启用）：
  - `fire_channel_read_raw_stream_segment(Bytes)`

**关键权衡：**
- segment 过小会导致 seg_count 上升，增加跨段 decode/copy 机会 → 需要阈值与统计证据。

### 阶段 C：后端侧真正提供“可移动”的 owned RX buffer
**思路：**
- 扩展 `DynChannel`：提供可选的 `rx_take_bytes(tok) -> Option<Bytes>`（或 `Vec<u8>`）
  - 若后端能把 read buffer 所有权移交给语义层，则阶段 B 的 segment append 真正零拷贝。
  - 否则仍走阶段 A 的 borrowed slice（保持正确性）。

**注意：**
- 这一步要与 future completion submit 的 buffer ownership contract 对齐，避免返工。

## 指标（先定义口径，再实现）
建议新增（P1 internal，不影响用户语义）：
- `rx_lease_tokens_total`
- `rx_lease_borrowed_bytes_total`
- `rx_materialize_bytes_total`（token 被 copy materialize 的字节数）
- `rx_cumulation_copy_bytes_total`（进入 tail copy 的字节数）
- `rx_zero_copy_segments_total`（push_segment 成功次数）

以及继续使用现有：
- `inbound_coalesce_acc` / `inbound_copied_bytes_acc`（多段 frame 的 coalesce 统计）

## 关键落点（代码索引）
- token materialize：`crates/spark-transport/src/async_bridge/channel_state.rs::materialize_rx_token`
- read loop：`crates/spark-transport/src/async_bridge/channel.rs::on_readable`
- stream slice fast-path：`pipeline/channel_pipeline.rs::fire_channel_read_raw_stream_slice`
- cumulation：`crates/spark-buffer/src/cumulation.rs`
- inbound decode：`crates/spark-transport/src/async_bridge/inbound_state.rs`
