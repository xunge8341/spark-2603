# Close semantics（P0 契约）

本文件冻结 dataplane 的 close / half-close / abort（RST/Reset）语义与证据事件。

## 目标

- 跨 OS/后端一致（IOCP/epoll/kqueue/WASI）。
- 不依赖“事件一定会来”（Readable/Writable/Closed 可能丢失/延迟）。
- 可审计：必须能通过 EvidenceEvent 还原 close 的原因与生命周期。

## 术语

- **CloseRequested**：本地请求关闭（显式 close 或内部 error shutdown）。
- **PeerHalfClose**：peer half-close（读到 EOF），读侧必须停止，但写侧允许继续 flush。
- **CloseComplete**：连接失活且资源回收完成（driver reclaim / channel_inactive）。
- **AbortiveClose**：RST/Reset 造成的 abortive close。

## P0 规则

1) Peer half-close（EOF）
- 读到 EOF 必须设置 `peer_eof=true` 并 PauseRead（避免 busy loop）。
- 写队列仍可 flush；driver 进入 draining，flush 完成后关闭。
- 必须发出 EvidenceEvent：`PEER_HALF_CLOSE`。

2) 显式 close
- 调用 close 必须：
  - PauseRead
  - best-effort 调用底层 `io.close()`
  - 发出 EvidenceEvent：`CLOSE_REQUESTED`
- 当连接最终失活并回收完成时，必须发出 EvidenceEvent：`CLOSE_COMPLETE(reason=requested)`。

3) I/O error 强制 close
- 任意 I/O error（含 Reset/Timeout/Internal）必须触发 **确定性 request_close**，不允许等待后端 Closed 事件造成僵尸连接。
- Reset 必须额外发出 EvidenceEvent：`ABORTIVE_CLOSE`。

## 观测口径

事件名与 unit_mapping 见：`docs/OBSERVABILITY_CONTRACT.md`（`UNITMAP_CLOSE_V1`）。

## Contract tests

- `spark-transport-contract/tests/close_evidence.rs`
- `spark-transport-contract/tests/abortive_close_evidence.rs`
