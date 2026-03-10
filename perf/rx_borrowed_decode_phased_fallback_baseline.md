# RX borrowed decode Phase-D fallback baseline

`SPARK_RX_BORROWED_FALLBACK_PERF` 不是吞吐 benchmark。

它的职责是钉住 borrowed fast path 的 fallback boundary：

- `leased >= 3`
- `borrowed_decode_attempt >= 3`
- `borrowed_decode_hit = 0`
- `borrowed_decode_fallback >= 3`
- `fallback_remainder >= 1`
- `fallback_partial >= 1`
- `fallback_decoder_policy >= 1`
- `materialize = 0`
- `lease_fallback = 0`

这表示：

- leased stream ingress 仍会发起 borrowed attempt；
- 但 remainder / partial / decoder-policy case 会 deterministically fallback 到 trunk owned path；
- Phase-D 冻结的是 **coverage boundary + evidence**, 不是扩大 borrowed lifetime。
