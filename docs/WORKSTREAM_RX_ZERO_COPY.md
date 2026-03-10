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
**终态边界：**
- 对 `ReadData::Token(rx)` 的 stream (`MsgBoundary::None`) 路径：
  - 使用 borrowed slice fast-path（仅当前 pipeline 调用栈有效）；
  - 严禁跨 handler/state/queue/异步任务保存 borrowed slice；
  - RAII guard 保证 `release_rx` 结构化且仅一次。
- datagram (`MsgBoundary::Complete`) 仍保持 owned 语义。
- 当存在 pre-frame handlers（`add_first`）时，强制走 owned fallback，保持可扩展语义。

**收益：**
- stream token 路径由“token materialize + cumulation copy”变为“borrowed slice + cumulation copy”；
- Phase A 明确终态是 **1 次 copy**（进入 cumulation tail），不是 0 次 copy。

**门禁：**
- contract suite 必须全绿（尤其 driver ordering / close semantics / framing roundtrip）
- 新增一个最小 contract：decode error/early return 也必须 release token exactly once（结构化 drop）

### 阶段 A contract（冻结） vs implementation strategy（可演进）

**Contract（必须稳定）：**
- token release must be **exactly once**，包含：normal success / decode error / early return / handler error。
- borrowed fast-path 只允许 stream（`MsgBoundary::None`）；datagram（`MsgBoundary::Complete`）必须走 owned。
- 有 pre-frame handler（`add_first`）时，不能走 borrowed fast-path，必须 fallback 到 owned 路径，行为与旧路径一致。
- framing roundtrip 与既有 decode/close/order contract 不回归。

**Implementation strategy（当前实现，可在不破坏 contract 前提下调整）：**
- stream token 路径当前仍会在 decoder/cumulation 处发生 1 次 copy；Phase A 不承诺 0-copy。
- pre-frame fallback 当前通过 `Bytes::copy_from_slice` 实现 owned 兜底，后续可替换为等价 owned 形态。

### 阶段 B：引入 “owned segment append” 关闭零拷贝闭环（从 1 次降到 0 次）
**终态边界：**
- 扩展 cumulation：新增 `Cumulation::push_segment(Bytes)`，用于把 **已拥有的 immutable segment** 直接入队（零拷贝）
- InboundState 增加 `append_stream_segment(Bytes)`：
  - 小 segment（< threshold）仍 copy into tail（减少碎片）
  - 大 segment 直接 push_segment（避免 copy）
- pipeline 增加 internal entrypoint（仅在无 pre-frame handlers 时启用）：
  - `fire_channel_read_raw_stream_segment(Bytes)`

**关键权衡：**
- segment 过小会导致 seg_count 上升，增加跨段 decode/copy 机会 → 需要阈值与统计证据。

### 阶段 C：后端侧真正提供“可移动”的 owned RX buffer
**终态边界：**
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
