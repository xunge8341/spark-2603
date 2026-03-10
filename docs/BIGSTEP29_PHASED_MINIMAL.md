# BigStep-29 / Phase-D minimal

目标：冻结 borrowed stream-decode 的 coverage boundary，而不是扩大 borrowed 生命周期。

## 本轮收口

- 只允许 **same-stack / exact single-frame** borrowed fast path 命中。
- 一旦出现以下任意情况，必须同步 fallback 到 trunk 现有 owned path：
  - decoder 已有 buffered remainder；
  - 当前 chunk 会留下 trailing remainder；
  - 当前 chunk 只是 partial frame；
  - 当前 decoder/profile policy 不允许 borrowed fast path。

## 新增稳定计数器

- `inbound_rx_borrowed_decode_fallback_remainder_total`
- `inbound_rx_borrowed_decode_fallback_partial_total`
- `inbound_rx_borrowed_decode_fallback_decoder_policy_total`

## Phase-D contract intent

- borrowed fast path 仍然局限在 **token-backed stream RX + same-stack decode**；
- 不把 borrowed 生命周期扩展到 app future / task / reclaim / generation；
- 复杂场景优先保持 trunk owned/cumulation 语义；
- `SPARK_RX_BORROWED_FALLBACK_PERF` 用来防止 borrowed boundary 被悄悄放大或 fallback reason 被抹平。
