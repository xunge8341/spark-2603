# UNSAFE Registry

本台账覆盖 `rg -n '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn)' crates -g '*.rs'` 的全部命中。

## crates/spark-transport/src/lease.rs

- **行为目的**：把后端 `RxToken -> (ptr,len)` 的短期借用转成可控的 `RxLease`，并在 drop 时结构化释放 token。
- **不能安全替代的原因**：上游接口暴露的是裸指针，`from_raw_parts` 与 `*mut LR` 解引用是 Rust 类型系统外部契约，需在边界处最小化封装。
- **前置不变量**：
  - `ptr..ptr+len` 在 `release_rx(tok)` 之前可读；
  - `reg` 在 lease 生命周期内始终有效；
  - `release_rx(tok)` 只执行一次。
- **违反后果**：越界读、UAF、double-release 导致后端状态损坏。

## crates/spark-transport/src/reactor/event_buf.rs

- **行为目的**：读取 `poll_into` 已初始化前缀中的事件。
- **不能安全替代的原因**：`MaybeUninit` 到已初始化值的读取必须使用 `assume_init_read`。
- **前置不变量**：`poll_into` 返回的 `n` 对应 `buf[0..n]` 已初始化。
- **违反后果**：读取未初始化内存，触发 UB。

## crates/spark-transport-iocp/src/native_completion.rs

- **行为目的**：调用 Win32 IOCP FFI（创建端口、投递完成、轮询完成、回收堆对象）。
- **不能安全替代的原因**：系统调用边界只能通过 `unsafe extern` API 与裸指针/句柄交互。
- **前置不变量**：
  - HANDLE 生命周期正确（创建后关闭一次）；
  - `Box::into_raw`/`Box::from_raw` 成对且仅一次；
  - `GetQueuedCompletionStatusEx` 返回 `removed` 前缀已初始化。
- **违反后果**：句柄泄漏、double-free、UAF、未初始化读取。

## crates/spark-transport-iocp/tests/native_completion_smoke.rs

- **行为目的**：从 `MaybeUninit` 读取轮询返回的前两个 completion 事件以验证回环。
- **不能安全替代的原因**：测试需要直接断言 `assume_init` 的初始化前缀语义。
- **前置不变量**：`poll_completions` 返回 `n == 2`。
- **违反后果**：读取未初始化内存。

## crates/spark-transport-contract/tests/framing_roundtrip.rs

- **行为目的**：构造测试用 no-op `Waker` 驱动 future。
- **不能安全替代的原因**：`RawWaker` / `Waker::from_raw` API 本身要求 `unsafe`。
- **前置不变量**：vtable 满足 RawWaker 契约，data 指针不被解引用。
- **违反后果**：唤醒路径 UB。

## crates/spark-transport-contract/tests/framing_roundtrip_fuzz.rs

- **行为目的**：同上，为 fuzz roundtrip 构造 no-op `Waker`。
- **不能安全替代的原因**：同上。
- **前置不变量**：同上。
- **违反后果**：同上。

## crates/spark-transport-contract/tests/rx_lease_observability.rs

- **行为目的**：同上，为 lease 观测测试构造 no-op `Waker`。
- **不能安全替代的原因**：同上。
- **前置不变量**：同上。
- **违反后果**：同上。

## crates/spark-buffer/src/scan.rs

- **行为目的**：按 8 字节块执行 `read_unaligned` 提升分隔符扫描吞吐。
- **不能安全替代的原因**：需要未对齐读取原始字节块；标准安全 API 不提供等价低层操作。
- **前置不变量**：
  - `i + 8 <= len` 时才读块；
  - 指针来自同一 `haystack` 切片。
- **违反后果**：越界读取 UB。

## crates/spark-buffer/src/bytes.rs

- **行为目的**：测试中验证 `slice` 后指针偏移与底层零拷贝语义。
- **不能安全替代的原因**：`p.add(3)` 是裸指针运算。
- **前置不变量**：原始缓冲区长度覆盖偏移 3。
- **违反后果**：越界指针计算 UB。
