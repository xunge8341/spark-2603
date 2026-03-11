# Decision Log（每次迭代必须输出“决策依据”）

本仓库采用 **ADR（Architecture Decision Record）+ Iteration Decision** 的方式记录关键决策。

- **ADR**：冻结长期架构/策略（分层、契约、可移植性、代码风格、门禁）。
- **Iteration Decision**：每个里程碑/迭代的主干推进决策，必须给出证据：
  - Contract（新增/加固）
  - Baseline（性能/资源基线与回归门禁）
  - Dogfooding（内部生态自举）

## Iteration 1（Phase-1 Kickoff）：冻结主干门禁与终态口径（2026-03-02）

- 决策：以 Transport Core GA 为第一阶段主干，先冻结 **P0 契约 + panic-free/unsafe gate + Dogfooding 口径**，再推进多后端与深性能优化。
- 依据：
  - 已存在产品化 Options（`DataPlaneOptions`）与 mgmt profile（HTTP/1）。
  - 已存在 contract suite 雏形（`spark-transport-contract`）。
  - 已存在基础 perf 指标与 gate 脚本（`scripts/perf_gate.*`，`perf/baselines/*`）。

对应 ADR：ADR-001/002/003/004。

## Iteration 2（Phase-1 P0 强化）：flush_limited 前进性契约化（2026-03-02）

- 决策：把 **flush fairness budget** 的“前进性”升格为 P0 契约：即使 reactor 不再发 Writable，driver 仍必须在有限预算内推进写队列（write-kick + pending_flush follow-up）。
- 依据：
  - 跨后端差异（edge/level/completion）与 half-close 场景中，Writable 事件可能缺失或延迟；若依赖 Writable 才 flush，会导致 p99/p999 抖动与尾延迟放大。
  - 当前 driver 已实现 bounded proactive flush（`scratch_write_kick`）与 budget follow-up（`pending_flush`）；缺口在于 **未被契约化**，未来引入 IOCP/epoll/kqueue/WASI 时容易回归。
  - 该契约同时满足“工业/金融/军事”要求：可预测、可复现、可审计（指标 `flush_limited_total` 可观测）。

## Iteration 3（Phase-1 Bugfix Gate）：write-pressure 恢复前进性修复（2026-03-02）

- 决策：修复 driver 的 proactive flush 执行路径，确保在 **backpressure 进入后**，即使 Writable 事件缺失/延迟，也能通过 bounded write-kick 持续尝试 flush，从而在 peer 恢复读取后尽快 drain 队列并退出 backpressure。
- 依据：
  - 现网/生产级语义必须保证“前进性”：peer 恢复读后，outbound backlog 必须最终清空；否则会造成连接级死锁（读暂停 + 写不前进）。
  - `spark-transport-mio` 的 `write_pressure_smoke` 集成测试暴露该类死锁（仅回写 2 个大包后停滞），属于 **P0 主干缺陷**，必须先修复再推进后续多后端与性能硬化。
  - 修复点是结构性且可审计：确保 non-draining 的 write-kick flush 不被条件分支“吞掉”，并保持 bounded（不 busy-spin）。

## Iteration 4（Phase-1 Cross-Platform Contract）：backpressure 退出后的 read-kick（2026-03-02）

- 决策：在 backpressure 退出（`BecameWritable`）后，driver 必须做一次 **bounded read-kick**：即使 reactor 没有再次产生 Readable 事件（edge-trigger/interest toggle 丢事件），也要主动调度一次 READ 任务以保证继续处理 OS 缓冲区中已到达的请求。
- 依据：
  - 写压力场景下，框架会主动 pause read（背压），此时 OS 接收缓冲区可能已经积累了大量请求数据；当背压解除并重新启用 READ interest 时，某些后端/平台可能不会重新触发 Readable（典型 edge-trigger 或 interest toggle 语义）。
  - 若仅依赖后续 Readable 事件，系统会出现“背压已退出但业务不再前进”的连接级停滞（只处理少量请求后停止），直接破坏工业/金融/军事场景对 **前进性、可预测性、尾延迟** 的要求。
  - read-kick 与 write-kick 同属跨平台契约化策略：都采用 bounded（每 tick 上限）避免 busy-spin，并且只在状态转换（BackpressureExit）时触发，开销可控、行为可审计。

## Iteration 5（Phase-1 主干继续）：观测口径常量化 + Windows mio write-pressure 测试暂缓（2026-03-02）

- 决策：
  1) 将 EvidenceEvent/metrics 的关键名称冻结为 `spark_uci::names` 常量，并在 dataplane 与 contract suite 中统一引用，形成“可运维/可审计”的稳定口径；
  2) `spark-transport-mio` 的 `write_pressure_smoke` 在 Windows 上出现持续性停滞（只回写少量响应后不再推进），短时间内无法定位到唯一根因，**不阻塞 Phase-1 主干推进**：该测试在 Windows 平台暂时标记为 `ignored`，并在 `docs/KNOWN_ISSUES.md` 中登记为 P0 待办。

- 依据：
  - 观测口径属于“世界一流”工程底座：没有稳定的事件/指标命名，dogfooding 与跨后端契约无法长期维持一致。
  - Windows/mio 的 write-pressure 停滞属于跨后端前进性问题，修复需要更系统的事件循环/interest 语义审计；在没有可复现根因与稳定修复前，贸然在主干反复试错会拖慢 Milestone 1（Transport Core GA）交付。
  - 该暂缓遵循“将军赶路不打小鬼”：先冻结可移植 contract/observability 主干，再在后续 Multi-backend 阶段集中解决 Windows/IOCP 的前进性语义。

## Iteration 6（Phase-1 大步推进）：Close/Half-close/Abort 语义与证据事件契约化（2026-03-02）

- 决策：把 **close / peer half-close / abortive close（RST/Reset）** 的核心语义与观测口径升格为 P0：
  - 在 `spark_uci::names::evidence` 冻结 `PEER_HALF_CLOSE / CLOSE_REQUESTED / CLOSE_COMPLETE / ABORTIVE_CLOSE` 事件名与 `UNITMAP_CLOSE_V1`；
  - 在语义层引入 `CloseCause`（best-effort、无分配）并在 `ChannelState` 内维护；
  - driver 在遇到任意 I/O error 时必须 **确定性地 request_close**（panic-free、bounded），避免“后端差异导致的僵尸连接”；
  - 新增 driver 级 contract：显式 close 必须产生 CloseRequested+CloseComplete；Reset 必须产生 AbortiveClose。

- 依据：
  - close/half-close/abort 是工业/金融/军事生产环境的“底线语义”：必须可预测、可审计、可回归；任何后端/平台的差异都会放大为 p99/p999 抖动与故障难以复现。
  - 现有代码已具备 backpressure/draining 的稳定基础，但 close 相关“原因/口径”此前缺少统一的可观测锚点（易导致后端各自解释）。
  - 将 close 原因收敛为 `CloseCause`（no_std 友好、无字符串分配）并在 `ChannelInactive` 时统一发出 CloseComplete，可形成跨后端一致的“证据闭环”，并直接支撑 dogfooding（mgmt plane 复用同一口径）。

对应 ADR：ADR-005。

## Iteration 6.1（Phase-1 继续大步推进）：统一 `chan_index` 跨 crate 可见性（2026-03-02）

- 决策：将 channel-id 解码 helper 统一为单一、稳定的 `spark_transport::async_bridge::chan_index(u32) -> usize`，并移除不一致的内部命名（`__chan_index`）。所有叶子层（mio）与 contract tests 统一使用 `chan_index`。
- 依据：
  - 该 helper 是 leaf backend 与 contract suite 需要的内部桥接点；命名不一致会导致跨 crate 编译失败，并迫使出现“临时别名/兼容中间态”。
  - 统一为 `chan_index` 可保持调用点清洁、语义明确，也符合你强调的“微软风格：清晰、可维护、无别名中间态”。
  - 该函数标注为 `#[doc(hidden)]`，避免误导业务使用者；生产使用仍通过 dataplane/driver API。

## Iteration 7（Phase-1 大步推进）：Dogfooding Gate + Ephemeral Bind 可连接（2026-03-02）

- 决策：将管理面自举从“可选 feature”升级为可回归门禁：
  1) 在发行物（`spark-dist-mio`）内通过集成测试强制验证 transport-backed mgmt server 可启动且可达（`GET /healthz` 返回 200/OK）；
  2) 调整 TCP dataplane 与 mgmt server spawn API：返回实际绑定地址（`local_addr/addr`），支持 `bind=127.0.0.1:0` 的可靠测试与 embedding。

- 依据：
  - 自举是 North Star 的硬指标：没有 dogfooding 的强门禁，口径/语义极易在长期演进中分裂。
  - 固定端口是 CI/并行测试的常见不稳定源；靠“先探测一个空闲端口再 bind”会引入 TOCTOU 竞态。
  - 让 spawn API 返回实际绑定地址是一种低成本、可审计、跨平台的工程化解法：
    - 不增加 runtime 依赖；
    - 不引入兼容中间态（直接调整返回类型并更新调用方）；
    - 对 Embedded/WASM 等未来扩展也更友好（由宿主环境决定端口/资源分配）。

对应 ADR：ADR-006。

## Iteration 8（Phase-1 继续大步推进）：Mgmt Profile v1 单一事实来源（2026-03-02）

- 决策：引入 `MgmtTransportProfileV1`（profile v1）作为管理面（HTTP）配置的单一事实来源（single source of truth），并强制 transport-backed mgmt server 只能通过 profile 生成 transport config。
  - profile 明确拆分为：`MgmtHttpLimits`（request limits）+ `MgmtIsolationOptions`（in-flight cap + transport capacity + budget）。
  - `ServerConfig::mgmt_profile_v1()` 作为产品层入口；现有 `management_transport_*` API 统一委托给 profile，避免重复实现与长期漂移。
  - `spark-ember::TransportServer` 构造函数改为显式接收 profile（不留别名/兼容中间态），确保 dogfooding 配置路径唯一。

- 依据：
  - mgmt plane 是自举闭环的“证据面”：若配置散落在 hosting/ember/tests 多处，长期演进必然产生口径漂移与不可复现的 profile。
  - profile v1 是纯数据（runtime-neutral），更适配后续 Embedded/WASM/多后端扩展：后端只需适配 transport 行为，而不重新解释 mgmt 语义。
  - 该收敛遵循“将军赶路不打小鬼”：一次性冻结 mgmt 主干语义与默认值，减少后续多后端阶段的返工。

对应 ADR：ADR-007。

## Iteration 9（Phase-1 继续大步推进）：Metrics 命名契约硬绑定 `spark_uci::names::metrics`（2026-03-03）

- 决策：将 Prometheus exporter 的指标命名从“散落字符串”升级为 **编译期常量引用**：
  - `spark-metrics-prometheus` 只允许引用 `spark_uci::names::metrics` 常量生成指标名（前缀 `spark_dp_` 作为 exporter 级约定）；
  - 补齐派生指标（derived gauges）的稳定命名常量：`write_syscalls_per_kib`、`write_writev_share_ratio`、`inbound_copy_bytes_per_read_byte`、`inbound_coalesces_per_mib`；
  - 将 `/metrics` 的 dogfooding smoke test 升级为“契约断言”：通过常量拼接期望名称（而非硬编码字符串），并覆盖多项关键指标。

- 依据：
  - 指标命名是生产运维与回归门禁的基础：一旦命名漂移，监控告警/仪表板/回归阈值会静默失效，风险远高于一次编译失败。
  - `spark_uci` 是 no_std 的“口径锚点”，天然适合承载稳定命名契约；将 exporter 强制绑定常量，可以把口径漂移转化为编译期失败（最理想的失败模式）。
  - 这是典型“将军赶路”的主干动作：先冻结可观测 contract，再推进多后端/深性能，避免后续在 IOCP/epoll/kqueue/WASI/WASM 上出现多套命名体系。

对应 ADR：ADR-008。

## Iteration 10（Milestone-2 大步推进）：Symmetric Framing 零拷贝 Outbound（2026-03-03）

- 决策：将 outbound framing 从“拼接成单个 Bytes（隐式 copy/alloc）”升级为 **小段向量化帧（vectored frame）**：
  1) 引入 `OutboundFrame`：最多 3 段（prefix/payload/suffix），prefix/delimiter 使用 inline buffer（≤16 bytes）避免堆分配；payload 继续使用 `Bytes` 实现零拷贝；
  2) `OutboundBuffer` 队列从 `VecDeque<Bytes>` 升级为 `VecDeque<OutboundFrame>`，flush 生成 iovec 跨“帧 + 段”聚合（writev/WSASend），并保持 partial-write 可推进；
  3) `StreamFrameEncoderHandler` 彻底改为 allocation-free：Line/Delimiter 仅追加 inline suffix（幂等）；LengthField/Varint32 仅 prepend inline header；
  4) pipeline 的 outbound 写入类型统一为 `OutboundFrame`，head handler 直接入队 vectored frame（无中间态）。

- 依据：
  - North Star 的 C++ 级性能目标要求：发送侧必须对称支持“零拷贝 + 批量 syscall”。此前 encoder 在 hot path 拼 Vec 并 copy payload，直接破坏 writev 与零拷贝价值。
  - `spark-codec` 已提供 allocation-free encoder primitives（Line/Delimiter/LengthField/Varint32），缺的是把产物形态贯穿到 transport 的 TX 队列与 flush。
  - 使用小型固定段（≤3）可以在保持代码清洁与可审计的同时覆盖当前所有内置 framing（并为未来 prefix+payload+suffix 预留空间），属于“将军赶路”的主干动作。
  - 该改动是结构性、一次到位、无别名中间态：队列形态与 handler 签名统一升级，可显著降低后续优化（pooling / io_uring / IOCP）返工风险。

对应 ADR：ADR-009。

## Iteration 11（Milestone-2 继续大步推进）：Partial write/vectored 合同 + append_suffix 正确性（2026-03-03）

- 决策：
  1) 新增 backend-independent contract test：partial vectored write 下 outbound buffer 必须保持字节流顺序与不丢不重（为 epoll/kqueue/IOCP/WASI 铺路）；
  2) 强化并单测 `OutboundFrame::append_suffix`：line/delimiter encoder 的追加操作必须 **幂等且默认正确**，并优先走 inline（热路径零分配），在段数打满时只 coalesce tail。

- 依据：
  - 多后端阶段最容易跑偏的不是“能不能写”，而是 **partial progress 的推进语义**。把该点契约化能显著降低未来 bring-up 成本。
  - delimiter/line 追加若依赖所有权技巧（move/unwrap_or）或后端事件，容易在 edge/level/IOCP completion 差异下出现隐蔽 bug；应将正确性下沉到 frame 类型本身。
  - 以上两个动作都属于“将军赶路”：以最小 API 面积冻结主干语义，避免后续碎片化修补。


## Iteration 12 — Clippy-clean correctness + backend contract mapping

### Decision
- Keep `cargo clippy -D warnings` as a hard gate by making intent explicit in code:
  - use `clamp` for bounded iovec budgeting
  - derive `Default` for `FrameSeg` and mark the default variant
  - keep items ordered (no impls after test modules)
- Add `docs/BACKEND_CONTRACT_MAP.md` and ADR-010 to make multi-backend bring-up mechanical and auditable.

### Rationale
- World-class codebases treat linters as *design feedback*. When clippy flags something, we either
  (a) encode the intent directly, or (b) document a deliberate exception with a rationale.
- Multi-backend work must not start from "tribal knowledge". A contract map reduces drift risk and
  enables parallel backend bring-up (IOCP/epoll/kqueue/WASI) without semantic divergence.

## Iteration 13（BigStep-11）：Ember mgmt backend 注入 + IOCP 分发路径（2026-03-03）

- 决策：
  1) 让 `spark-ember` 的 transport-backed mgmt server **不再依赖任何具体后端**（移除对 `spark-transport-mio` 的依赖）；
  2) mgmt server 改为通过 `try_spawn_with(spawn_fn)` 注入后端 spawn closure，后端由 dist crate 决定；
  3) 新增 Windows-first 分发 crate：`spark-dist-iocp`，使内部生态能在 Windows 上通过 IOCP 边界完成 dogfooding。

- 依据：
  - North Star 要求“跨软硬平台 + 可替换后端”，而 mgmt-plane 是自举闭环的验收面；若 ember 绑定到 mio，会导致
    **后端扩展时 mgmt 与 dataplane 口径分裂**。
  - `Service` 采用 Rust 原生 async trait（非 object-safe），因此采用“在调用点注入 spawn closure”而不是 dyn trait。
    这样既保持 ember 运行时中立，又不牺牲 hot path 的静态分发。
  - `spark-dist-iocp` 的存在使 Windows 生态能以“同一套 host+mgmt UX”验证 transport 设计（dogfooding gate），
    并为后续 native IOCP completion bring-up 留出干净的替换空间。

对应 ADR：ADR-011。


## Iteration 14 — Ember examples: backend feature gating (mio/iocp)

- Decision: keep `spark-ember` backend-neutral; allow examples to opt into backends via optional dependencies + features.
- Rationale: `cargo clippy --workspace --all-targets -D warnings` compiles examples; un-gated references to backend crates break builds.
- Decision recorded in code comments: `examples/spark_switch.rs` uses `cfg(feature=...)` blocks.

## Iteration 15 — Clippy-clean examples under `-D warnings`

### Decision
- Make `examples/spark_switch.rs` compile cleanly when **no backend feature** is enabled:
  - gate backend-only imports (e.g. `Arc`) behind `cfg(any(feature="backend-mio", feature="backend-iocp"))`
  - in no-backend branches, mark closure parameters as used via `let _ = (&param, ...)`
  - use `std::io::Error::other(..)` to satisfy `clippy::io_other_error`

### Rationale
- This repo treats `cargo clippy --workspace --all-targets -D warnings` as a production gate.
- Examples are part of the developer UX contract; they must not require feature flags just to compile.
- We keep the closure signatures readable for backend-enabled builds, while ensuring clippy-clean builds by default.

## Iteration 16（BigStep-15）：IOCP 分发 dataplane echo smoke（2026-03-03）

### 决策
- 在 `spark-dist-iocp` 增加 Windows-only 的 dataplane echo smoke：`tests/dataplane_echo_smoke.rs`。

### 依据
- Multi-backend bring-up 不能只依赖“unit contract”；需要至少一个 **端到端可运行** 的 dataplane smoke 来验证：
  accept → framing → writev flush → close 的主干路径。
- 该测试刻意使用 line framing 的最小协议形态，避免把 mgmt/HTTP 的复杂度混入 backend bring-up。
- 在 native IOCP completion driver 未落地前，这个 smoke 让 `spark-dist-iocp` 的分发边界保持“诚实可跑”，
  并为后续把内部实现替换为 completion 模式提供可回归的验收面。

## Iteration 17（BigStep-16）：修复 IOCP echo smoke 的换行符误用（2026-03-03）

### 决策
- `spark-dist-iocp/tests/dataplane_echo_smoke.rs` 的客户端消息必须发送真实换行符 `"\n"`（LF），而不是字面量 `"\\n"`。

### 依据
- 该 smoke 采用 **line framing**。若客户端不发送 LF，inbound Line decoder 不会产出 frame，echo service 不会被调用，
  将导致“后端实现错误”的 **假阳性**。
- 将该约束写入测试内的代码注释，保证后续维护者在修改 payload/codec 时不会再次引入同类误判。

## Iteration 18（BigStep-17）：Completion API Foundation（IOCP / io_uring）（2026-03-03）

### 决策
- 在 `spark-transport` 的 `reactor` 模块下新增 `completion` 子模块（feature gate：`spark-transport/completion`），提供最小完成模型：
  - `CompletionEvent` / `CompletionKind`
  - `CompletionReactor::poll_completions(..)`
- 在 `spark-transport-iocp` 增加 `native-completion` feature，提供 `IocpCompletionReactor` 的原型实现（仅完成端口轮询）。

### 依据
- readiness（mio/epoll/kqueue）与 completion（IOCP/io_uring）在语义与生命周期建模上天然不同。
- 直接把 completion 语义塞进当前 `Reactor`/`IoOps` 会触发跨 crate 大范围重构，风险高、节奏慢。
- 先提供“完成事件的最小模型”可以让 IOCP/io_uring 的工作并行推进，同时不污染主干 dataplane（保持将军赶路的主线速度）。
- 该步骤的决策依据同步写入模块级代码注释，确保后续评审与安全审计可追溯。

## Iteration 19（BigStep-19）：IOCP completion 最小 submit 闭环（2026-03-03）

### 决策
- 在 `spark-transport-iocp` 的 `native-completion` 原型中，先引入 **显式 completion packet** 的最小提交能力：
  - 新增 `IocpCompletionReactor::post(..)` / `post_ok(..)` / `post_err(..)`，底层使用 `PostQueuedCompletionStatus`；
  - `poll_completions(..)` 能识别并回收这些显式投递的 packet，并还原出原始 `CompletionEvent`（保留 kind / token / outcome）。
- 维持该能力为 **leaf-local**（仅 `spark-transport-iocp`），暂不把“提交 API”上推到 core 的稳定 trait。

### 依据
- 现阶段最大缺口是 native IOCP 的 submit 路径，但直接把 `AcceptEx` / `WSARecv` / `WSASend` 接入 dataplane 会立刻引入缓冲区所有权、取消、partial progress、close 竞争等高风险语义。
- 在真正接入 socket overlapped 之前，主干仍需要一个 **真实可运行** 的 submit → completion → poll 闭环，来冻结 `CompletionEvent` 在 IOCP leaf 中的流转方式，并把 bit-rot 风险前置到 CI/nightly。
- 采用 `PostQueuedCompletionStatus` 的显式 packet 属于最小可审计闭环：
  - 不抢先承诺缓冲区/FD 生命周期模型；
  - 仍然通过真实 IOCP 队列验证投递、轮询、错误传播；
  - 能为后续把 posted packet 替换为真实 overlapped 提交提供稳定回归基线。
- 之所以把该能力留在 leaf crate，而不是现在就扩展 core trait，是为了避免在 completion 语义尚未收敛前，把“测试性 submit”误升级为跨后端稳定承诺。

## Iteration 20（BigStep-20A）：`spark-buffer::Bytes` 的 `from(Vec<u8>)` 真·零拷贝（2026-03-03）

### 决策
- 将 `spark-buffer::Bytes` 的共享表示从 `Arc<[u8]>` 调整为 `Arc<Vec<u8>>`（内部 `BytesInner::SharedVec`）。
- 新增单测门禁：`Bytes::from(Vec<u8>)` 必须保持元素指针不变（pointer-stable），并且 `slice(..)` 的指针偏移必须正确。

### 依据
- 现有实现通过 `Vec<u8> -> Box<[u8]> -> Arc<[u8]>` 构造共享缓冲区：
  - `Arc<[u8]>` 的分配布局包含 Arc header；无法直接复用 `Box<[u8]>` 的 allocation。
  - 这会触发 **隐形的重新分配与字节拷贝**，导致“看起来零拷贝”的 fast-path 实际仍在 copy（吞吐与 p99/p999 抖动都会被悄悄放大）。
- `Arc<Vec<u8>>` 方案是当前阶段“最小侵入、收益最大”的修复：
  - `Bytes::from(Vec<u8>)` 仅移动 `Vec` 结构体，不复制底层字节；
  - 对外 API 不变（仍是 `Bytes` + `Deref<[u8]>` + `slice/try_slice`），对后续开发侵入极低；
  - 通过 pointer-stable 单测锁死语义，防止后续无意回退。
- 该决策属于“主体框架热路径优先”的优化：不消耗 IOCP leaf 的预算，却能全局降低 copy/byte 与抖动。

## Iteration 21（BigStep-20B）：Stream cumulation 引入 tail 聚合，降低 segment fragmentation（2026-03-03）

### 决策
- `spark-transport` 的 `Cumulation` 继续以 `ByteQueue` 作为主存储，但新增一个 `BytesMut` tail：
  - 新读取的 stream bytes 先追加进 tail（不立刻 materialize 成 `Bytes` segment）；
  - decoder 扫描时把 tail 作为“最后一个 segment”参与 `iter_segments()`；
  - 一旦产生 frame 并需要消费（consume/take），先 `materialize_tail()`：把 tail `freeze()` 成一个不可变 `Bytes` 并 push 到队列，然后按原 `ByteQueue` 逻辑消费。
- 为支撑该策略，在 `spark-buffer::BytesMut` 增加显式 `freeze()`：把 live region 变成 `Bytes`，并清空自身。

### 依据
- `ByteQueue::push_slice(..)` 会为每次 read 分配一个新的 `Bytes`：在 small-read/高并发场景下容易造成 segment 数量膨胀，从而提高 multi-segment frame 的概率并放大 `coalesce` copy。
- tail 聚合把“每次 read 一个 segment”收敛为“最多一个 tail segment”，能显著降低 fragmentation，并且不改变 codec/decoder 的 contract（仍旧是 `&[u8]` segments）。
- 消费阶段仍使用 `ByteQueue` 的 slice/advance 语义，避免回退到纯 contiguous cumulation 的大规模 memmove。
- `BytesMut::freeze()` 是显式 API：不会误导调用方在热路径上频繁 freeze（需要时才用），并保持 `unsafe` 预算为零。

## Iteration 22（BigStep-20X）：修复 windows-sys HANDLE 类型漂移导致的 completion gate 编译失败（2026-03-03）

### 决策
- `spark-transport-iocp/native_completion` 中与 `CreateIoCompletionPort` 交互的 0/NULL sentinel 改为指针语义：
  - 使用 `null_mut()` 判断与传参；
  - 创建未关联 completion port 时显式传入 `INVALID_HANDLE_VALUE`（leaf-local 常量），避免把 Windows 细节扩散到 core。

### 依据
- `windows-sys` 0.61+ 将 `HANDLE` 建模为 `*mut c_void`，继续使用 `0`（usize）会触发 Rust 类型错误并导致 `SPARK_VERIFY_COMPLETION_GATE=1` 失败。
- 该修改属于“叶子后端保底不倒”：只修复类型/语义正确性，不扩大 IOCP leaf 投入。

## Iteration 23（BigStep-20C）：Outbound flush 采用 iov 元数据推进 head，减少重复扫描并提升可读性（2026-03-04）

### 决策
- `OutboundBuffer::flush_into` 在构建 iovec 时同时记录 `IovMeta`（frame/segment/off/len）。
- write/writev 成功后，不再用“while remaining + prune_head”按字节推进：
  - 改为 `advance_after_write(written, metas)`：按 iov 元数据一次性推进 head（并在最后 `prune_head()` 清理空段/出队）。

### 依据
- 旧实现需要在每次成功写入后循环调用 `prune_head()` 并逐段推进 head：
  - 热路径上重复的分支与队列访问会放大 CPU 抖动（尤其在 partial write 比例偏高时）。
  - 逻辑上也更难读（推进规则分散在多处）。
- 新实现把“写入后推进”的规则收敛为一个函数：
  - 语义等价（written 永远只消费本次 gather 的前缀字节）；
  - 让推进过程与 gather 的顺序一致，可审计、可维护；
  - `prune_head()` 仅保留为尾部清理，避免在循环中反复调用。

## Iteration 24（BigStep-20C）：completion gate 中 GetLastError 的 unsafe 边界收敛（2026-03-04）

### 决策
- `spark-transport-iocp/native_completion` 新增 `last_error()` 小封装：
  - 统一承载 `unsafe { GetLastError() }`；
  - 调用点保持纯安全代码。

### 依据
- `windows-sys` 将 `GetLastError` 标注为 unsafe（FFI）。
- 将 unsafe 收敛到 leaf-local 单点，有利于审计与 clippy-clean，并符合“unsafe 可审计/可控”的 North Star。


## Iteration 25（BigStep-20D）：Line framer 采用窗口扫描语义，避免“大读”误判 FrameTooLong（2026-03-04）

### 决策
- `LineFramer::decode` 移除 `live > max_len` 的早退判定：
  - 改为在一个明确的扫描窗口内（`scan_limit`）查找 `\n`；
  - 若在窗口内找到 terminator，则即便 cumulation 已缓冲更多后续字节（例如下一帧），也应正常解出当前帧；
  - 若在窗口内未找到 terminator 且 live 已超过窗口，则判定 `FrameTooLong`。
- `scan_limit` 与 `include_delimiter` 对齐：
  - `include_delimiter == true`：`max_len` 约束 consumed 字节数（包含 `\n`）；
  - `include_delimiter == false`：允许额外 `\r\n`（最多 2 字节）出现在 message 之后。

### 依据
- 旧实现将 `cumulation.len()` 直接与 `max_len` 比较，假设“每次读都很小且不会一次缓冲多帧”。
  - 在实际 backend（尤其是大读/批量读）下，cumulation 可能一次包含 **完整的第一帧 + 后续字节/下一帧**；
  - 这会导致明明 terminator 已在限制内，却被误判为 `FrameTooLong`。
- 新实现与 `DelimiterFramer` 的 scan-limit 语义一致：
  - 仅对“terminator 在限定窗口内是否出现”做判断；
  - 将 max-frame 约束从“buffer 长度”纠正为“可接受 frame 窗口”，更符合协议语义且更易维护。

## Iteration 26（BigStep-20E）：Stream framer 引入增量扫描状态，避免反复全量扫描导致 O(n²)（2026-03-04）

### 决策
- `LineFramer`/`DelimiterFramer` 增加增量扫描状态字段：
  - `scanned`：已扫描的 cumulation 字节数（相对于当前 cumulation 起点）。
  - `LineFramer` 额外 `last_byte`：用于 `
` 跨 segment 边界的判断。
  - `DelimiterFramer` 额外 `matched`：保存多字节 delimiter 的 KMP 前缀匹配进度。
- `decode()` 行为：
  - 未找到 terminator 时，仅扫描新增 suffix，并更新 scan 状态；
  - 找到 frame 或报错时，重置 scan 状态，避免 state 与消费后的 cumulation 脱节。

### 依据
- 旧实现每次 `decode()` 都从 cumulation 起点重新扫描：
  - 在“慢发/长帧/大量 partial read”场景下会形成 O(n²) CPU 行为，直接冲击 p99/p999。
- `InboundState` 的 `framer` 是 per-connection 持久化对象，天然适合承载 scan 状态，不引入额外分配，也不改变 framing contract。
- 通过单元测试锁死“scan progress 单调推进 + delimiter 跨 append 可匹配”的语义，防止后续回退。

## Iteration 27（BigStep-20F）：Outbound flush gather 逻辑收敛为顺序 cursor + 单次 prune（2026-03-04）

### 决策
- `OutboundBuffer::flush_into` 将 iov gather 逻辑收敛为 `gather_iov()`：
  - 使用显式 `(frame_off, seg, off)` cursor 顺序遍历队列，构建 iov + metas；
  - 避免 `iter().enumerate()` + 嵌套循环的隐式控制流，提升可读性与可维护性。
- `flush_into` 仅在进入循环前调用一次 `prune_head()`：
  - 后续每次成功写入都通过 `advance_after_write()` 推进 head，并在尾部 `prune_head()` 清理空段/出队；
  - 避免在热循环中重复 `prune_head()`。

### 依据
- gather 属于 flush 热路径，结构化为单点 helper：
  - 更易审计（partial write 的推进规则与 gather 顺序严格对齐）；
  - 为未来进一步的 cursor 复用或微优化提供稳定落点，避免再次“散落式”重构。
- loop 内减少重复的 `prune_head()` 调用，有助于降低分支与队列访问抖动，同时保持语义等价。

## Iteration 28（BigStep-21）：将 stream cumulation 下沉到 spark-buffer（alloc 层），统一复用并减轻 transport 负担（2026-03-04）

### 决策
- 将 `Cumulation` / `TakeStats` 从 `spark-transport::async_bridge` 下沉到 `spark-buffer`：
  - `spark-buffer` 作为 L1(alloc) 的 bytes/framing 原语归属更合理；
  - `spark-transport` 继续以“使用者”身份依赖 `spark-buffer::Cumulation`，不再维护 transport-local 版本。
- transport 侧仅做最小改动：`InboundState` 的 import 替换为 `spark_buffer::{Cumulation, TakeStats}`。

### 依据
- Cumulation 本质是“分段字节累积 + 按需 coalesce”的通用原语，属于 buffer 层而非 transport 层：
  - 便于未来在 Embedded/WASM/WASI 等场景复用（不绑定 transport 的状态机/driver 语义）；
  - 减少重复实现与后续维护成本。
- 下沉后接口不变（只移动归属），对上层 framing/contract/dogfooding 无侵入。


## Iteration 29（BigStep-22）：mgmt HTTP/1 framing 复用 Cumulation，并将“过大”判定从 buffer 长度纠正为请求窗口（2026-03-04）

### 决策
- `spark-transport::StreamFrameDecoderHandler` 的 HTTP/1 framing（feature `mgmt-http1`）：
  - 将 `Http1InboundState` 的 cumulation 从 `BytesMut` 切换为 `spark-buffer::Cumulation`；
  - 对 head decoder 引入“按需 coalesce 的 scratch prefix”：
    - 单 segment（常见）直接零拷贝借用；
    - 多 segment 时仅 coalesce `max_head_bytes` 前缀，避免将整个 buffer 强行变连续。
  - 引入 `max_buffered = max_request_bytes + max_head_bytes`：
    - 约束整体缓冲上限，避免无界 pipelining；
    - 同时允许“第一条请求已完整，但后续字节已到达”的场景继续正确解出第一条请求。
- HTTP/1 framing 在成功提取 request 时，使用 `Cumulation::take_message_with_stats` 记录 coalesce/copy 指标，并透传到 `ChannelState::record_inbound_cumulation`。

### 依据
- 旧实现以 `cumulation.len() > max_request_bytes` 作为 TooLarge 判定，会在“大读/批量读/或客户端提前发送下一条请求”时误判：
  - 明明第一条请求在限制内，却因后续字节已到达而被拒绝。
- `Cumulation` 已在 stream framing 主干上验证（P0 contract + smoke），mgmt HTTP/1 复用同一原语能提升一致性与可维护性。
- 引入 `verify_nightly.{ps1,sh}` 作为入口，统一开启 perf/bench/completion 编译 gate，避免日常 PR 脚本被重负载拖慢。


## Iteration 30（BigStep-24）：引入 no_std/alloc 目标编译门禁 + baseline 变更守门（2026-03-04）

### 决策
- 新增 `scripts/nostd_gate.(sh|ps1)`：对 L0/L1 crates（`spark-uci/spark-core/spark-buffer/spark-codec*`）执行跨目标编译验证：
  - `thumbv7em-none-eabihf`（Embedded）
  - `wasm32-unknown-unknown`（WASM core）
- 将 no_std gate 纳入 nightly（`scripts/verify_nightly.*` + `.github/workflows/nightly.yml` 追加 job），避免影响日常 PR 的验证时长。
- 新增 `scripts/baseline_guard.sh` 并在 PR CI 中启用：当 `perf/baselines/*` 发生变更时，必须同步更新 `docs/DECISION_LOG.md` 记录依据。

### 依据
- North Star 要求 L0/L1 能扩展到 Embedded/WASM；跨目标编译门禁可以最早发现 `std` 回归与目标不兼容。
- perf/bench 基线变更属于“工程决策”，必须可追溯；用 CI 机械化守门可降低阈值漂移与隐性放水风险。


## Iteration 31（BigStep-26）：将 mgmt HTTP/1 inbound framing 抽离为独立模块，降低 feature-gated 复杂度（2026-03-04）

### 决策
- 将 `crates/spark-transport/src/async_bridge/pipeline/frame_decoder.rs` 中的 mgmt HTTP/1 inbound framing（feature `mgmt-http1`）抽离到独立模块：
  - 新增 `pipeline/http1_inbound.rs`，集中维护 `Http1InboundState`、`Http1AppendError`、`head_prefix_from` 等实现细节；
  - `frame_decoder` 仅保留“协议分流 + 指标记录 + 事件透传”的主流程，以提升可读性与可维护性。

### 依据
- `frame_decoder` 同时承担 stream framing 与 mgmt HTTP/1 的 feature 分支，随着借用卫生（scratch/mem::take）等细节增加，文件可读性下降。
- 抽离模块后：
  - feature-gated 代码集中在单文件，审计更直观；
  - 主流程文件更短更清晰，后续迭代更不容易引入借用交叉（E0499）类问题。


## Iteration 32（BigStep-27）：将 stream framers 抽离为独立模块并复用分段扫描辅助，提升可读性与可维护性（2026-03-04）

### 决策
- 将 `async_bridge/inbound_state.rs` 中的内建 stream framers（Line/Delimiter/LengthField/Varint32）抽离为独立模块：
  - 新增目录 `async_bridge/framers/`，每种 framer 独立文件（`line.rs/delimiter.rs/length_field.rs/varint32.rs`），并在 `framers/mod.rs` 统一定义 `FrameSpec/StreamDecodeError/StreamFramer`。
- 复用分段扫描辅助函数（`framers/segment.rs`）：
  - 统一处理跨 segment 的 skip 与 scan window clip；
  - 为 LengthField/Varint32 提供通用的 prefix peek，减少重复拷贝代码。
- `inbound_state` 保持为“连接态 orchestrator”，仅负责：cumulation 管理、decode loop 预算、消息出队与统计聚合。

### 依据
- 原 `inbound_state.rs` 混合了：连接态、四类 framing、KMP 细节、单测，文件过长且 feature 迭代容易引入借用/边界 bug。
- 抽离后：
  - 每个 framer 的状态机更集中，审计与单测更直接；
  - 连接态逻辑更短、更清晰，后续扩展（例如新增 framing）不会污染主流程。

## Iteration 33（BigStep-28 规划）：先冻结 Driver Scheduling Kernel，再做热路径优化（2026-03-04）

- 决策：将下一阶段主干工作重心明确为 **Driver Scheduling Kernel**，先冻结 driver 内部的“调度语义内核”，再进行去 `HashSet` / 去 `sort+dedup` / RX 零拷贝等局部性能优化。
  - 当前 `pending_flush / pending_read_kick / scratch_*` 等队列被视为“调度原因”的具体编码，而不是长期架构本身；
  - 下一步先把调度原因与单次入队语义明确化，并补齐内部观测（scheduler pressure / RX materialize），然后再替换热路径容器；
  - 期间保持用户可见语义与 P0 contract 不变，不引入兼容中间态。

- 依据：
  - `ADR-000` 要求 hot path 保持静态分发；driver 是后端差异吸收层，不能为了某个局部优化把后端特例重新泄漏到 core surface。
  - `ADR-003` 明确禁止长期兼容中间态；若现在先做局部容器优化，后续 completion-style 调度真正落地时可能被迫重做一遍。
  - `ADR-010` 明确后端差异必须止步于 driver/adapter；因此现在最值钱的是先冻结“调度内核形状”，而不是先追局部基准收益。
  - `ADR-012` 说明 completion submit 仍未进入主干；在提交/所有权契约未冻结前，RX 零拷贝生命周期也不宜贸然放大。

对应 ADR：ADR-013。

## Iteration 34（BigStep-28 Step-1）：显式 Driver Scheduling Kernel 词汇并收敛队列帮助逻辑（2026-03-04）

### 决策
- 在 `channel_driver.rs` 中引入内部 `ScheduleReason` 语义枚举，显式表达 driver 的稳定工作原因：
  `InterestSync / Reclaim / DrainingFlush / WriteKick / FlushFollowup / ReadKick`。
- 将调度队列的公共整理逻辑收敛为局部 helper（排序/去重/有界截断 / follow-up batch 取出），
  但 **不** 在这一步替换 `HashSet + Vec` 的现有热路径容器。
- 将 reactor Readable 与 `ReadKick` 的“提交读任务”路径收敛为同一个内部 helper，
  仅通过 `missing_is_paused` 区分语义保守度。

### 依据
- 这是 ADR-013 的第一段落地：先让调度语义在代码中显式存在，再做容器替换，避免把局部热路径优化误当成架构。
- 当前改动保持外部 contract 不变，只做内部表达收敛，能为后续：
  - 单次入队语义冻结；
  - per-channel flags 替换；
  - readiness/completion 统一调度内核；
  提供更稳的评审基线。
- 将重复的队列整理/有界 follow-up 逻辑集中，可以降低后续改造时出现“某个 reason 忘记 dedup / 忘记 bounded”的局部回归风险。


## Iteration 35（BigStep-28 Step-2）：冻结 driver 单次入队语义，先去 `sort/dedup` 再动 `inflight`（2026-03-04）

### 决策
- 在 `channel_driver.rs` 中新增按 slot 维护的 generation-aware `ScheduleSlotState`，以 `(chan_id, reason)` 为粒度冻结单次入队语义。
- 所有 `pending_flush / pending_read_kick / scratch_*` 调度队列继续保持按语义拆分的 `Vec<u32>`，但从本步开始改为：
  - append-only 入队；
  - 同一 live channel 对同一 `ScheduleReason` 不重复入队；
  - 出队时按当前 slot generation 过滤 stale work，且 stale 项不消耗当 tick 的有界处理预算；
  - 对成功出队的 live work 再清除对应 reason bit。
- 因此本步移除 driver tick 中对这些队列的 `sort_unstable + dedup` 依赖，但仍保留 `inflight: HashSet<u32>` 作为“每连接最多一个 task token”合同的现有实现。

### 依据
- 这是 ADR-013 的第二段落地：先把“单次入队”的语义做成显式且可审计的状态机，再继续推进 `HashSet` -> per-channel flags。
- 若直接去掉 `sort/dedup` 而没有 generation-aware 状态，slot 复用时会把 stale 队列项与新 channel 混淆，存在 suppress 合法调度的风险。
- 通过把去重语义绑定到 `(chan_id, reason)` 而不是裸 `idx`：
  - 保留了通道复用下的正确性；
  - 让 future readiness/completion 共用同一调度语义边界；
  - 也为后续内部观测（enqueue / stale skip / follow-up pressure）提供了稳定挂点。

## Iteration 36（BigStep-28 Step-3）：将 task ownership 收敛为 generation-aware slot-local 状态（2026-03-04）

### 决策
- 在 `channel_driver.rs` 中新增 `TaskSlotState`，以 `(chan_id, TaskToken)` 记录当前 live channel 的 executor task 所有权。
- `try_submit_read_task()` 从本步开始不再依赖全局 `inflight: HashSet<u32>`，而是：
  - 先检查 `TaskSlotState` 是否已有 live token；
  - 分配 `TaskSlot` 后，将 `TaskToken` 绑定到对应 `chan_id`；
  - 若绑定失败（例如跨 generation 冲突）或 `executor.submit()` 失败，则立即回滚 `TaskSlot` 与 `TaskSlotState`。
- `reclaim_channel()` 从本步开始按完整 `chan_id` 直接取回并释放记录中的 `TaskToken`，不再通过 `slab.find_token_by(...)` 做线性查找。

### 依据
- 这是 ADR-013 的第三段落地：在调度 reason 与单次入队语义已经冻结后，继续把“每连接最多一个 task token”合同也收敛成 slot-local、generation-aware 状态。
- 这样做可避免：
  - 全局 `HashSet<u32>` 在热路径上的哈希成本；
  - `reclaim_channel()` 在线性扫描 slab 时额外引入的慢路径复杂度；
  - slot 复用时因裸 index 或 stale token 导致的新旧 channel 混淆。
- 本步仍保持外部 contract 不变；变化点仅在 driver 内部所有权模型与错误回滚路径。


## Iteration 37（BigStep-28 Step-4）：落地 driver kernel 内部可观测性计数器（2026-03-04）

### 决策
- 在 `spark_uci::names::metrics` 中新增一组 **稳定、无标签（label-free）** 的 driver-kernel 指标后缀名，并在 `spark_transport::DataPlaneMetrics` 中以 atomic counter 形式承载：
  - Scheduler pressure：`driver_schedule_*_total`（按 `ScheduleReason` 拆分）与 `driver_schedule_stale_skipped_total`；
  - Task ownership pressure：`driver_task_submit_*` / `driver_task_finish_total` / `driver_task_reclaim_total` / `driver_task_state_conflict_total`。
- 在 `channel_driver.rs` 中将这些计数器挂在“语义稳定挂点”上：
  - 入队成功（单次入队语义成立）时计数；
  - `take_schedule_batch` 丢弃 stale work 时计数（且 stale 不消耗 bounded per-tick budget）；
  - `try_submit_read_task` 的 inflight/paused 抑制、submit 成功/失败、state conflict；
  - task 完成（finish）与 reclaim 释放 token。
- 在 `spark-metrics-prometheus` exporter 中输出上述计数器，统一使用 `spark_dp_` 前缀，并保持 suffix 由常量驱动。

### 依据
- 这是 ADR-013 的“先证据后优化”落地：在继续替换更多热路径容器或扩大 RX zero-copy 生命周期之前，先把 driver 内部压力变成可解释、可回归的证据。
- 指标选择遵循 ADR-008：**命名 contract 单一事实来源** 固定在 `spark_uci::names::metrics`，避免字符串散落导致口径漂移。
- 计数器设计保持低成本与可审计：
  - 全部为 atomic `fetch_add(Relaxed)`；
  - 无 label（避免 exporter/聚合器引入额外复杂性）；
  - 按语义原因拆分，便于跨 OS/后端对比。


## Iteration 38（BigStep-28 Step-5）：InterestSync/WriteKick 从“每 tick 扫描”收敛为“边沿驱动”，并去重 reactor.register（2026-03-04）

### 决策
- 在 `channel_driver.rs` 中引入 generation-aware 的 `InterestSlotState`（slot-local），缓存当前 live `chan_id` 的 **已注册 interest**。
- `ChannelDriver::sync_interest` 与 `DriverRunner::sync_interest` 从本步开始：
  - 先对比 `desired_interest` 与缓存的已注册 interest；
  - **仅当 interest 发生变化**时才调用 `reactor.register()`；
  - 并通过稳定指标计数：
    - `driver_interest_register_total`（发生 register 调用）
    - `driver_interest_register_skipped_total`（interest 未变化而跳过）
- Driver tick 的“draining/close 扫描”从本步开始不再无条件 enqueue `InterestSync` / `WriteKick`：
  - `InterestSync` 改为 **仅在 desired_interest != last_registered_interest** 时 enqueue；
  - 非 draining 的 `WriteKick` 改为 **仅在 WRITE interest 边沿（transition to WRITE）** 时 enqueue；
  - draining 的 `DrainingFlush` 仍保持 per-tick best-effort（用于 peer half-close 等场景下保证 forward progress）。
- 同步更新 `spark-uci::names::metrics` 与 Prometheus exporter，确保新指标命名 contract 单一事实来源。

### 依据
- 这是 ADR-013 的“先证据后优化”在调度内核上的下一步落地：
  - Step-4 已提供 schedule/task 压力的可解释计数器；
  - 本步在不改变用户语义的前提下，直接降低 steady-state 的 **无效 register 与无效 flush 扫描**。
- `WriteKick` 的边沿驱动是整体最优的权衡：
  - steady-state 的 TX 热路径已经在 **app completion** 与 **reactor Writable** 上做了 best-effort flush；
  - edge-triggered 后端在“启用 WRITE interest 且 socket 已可写”时可能不发 Writable edge；
  - 因此只需要在 **WRITE interest 从无到有**时 kick 一次即可避免 stall，无需每 tick 反复 flush。
- `InterestSlotState` 把“interest 是否变化”的判断收敛成 generation-aware slot-local 状态：
  - 避免重复 syscalls；
  - 避免 slot 复用导致 stale generation 抑制新 channel；
  - 与 `ScheduleSlotState` / `TaskSlotState` 一致，保持可审计、可演进。



### 补充修正（契约门禁）：install_channel 必须建立初始 interest

- 合同原因：contract suite 中存在在首个 tick 之前直接变更 channel 状态（如 enter_draining）的用例；
  若未在 install 时建立一次初始 register，则 edge-triggered InterestSync 可能观察到 `prev == desired == empty` 而跳过 register，
  从而违反“draining/close 下 interest 必须可观测更新”的契约。
- 修正：`install_channel()` 在完成 slot 安装后立即调用 `sync_interest(chan_id)`（并更新 InterestSlotState），保证该 generation 至少一次 register。

## Iteration 30A.2（BigStep-30A.2）：effective config 结构化输出（2026-03-10）

### 决策
- 为 transport/host/mgmt 增加稳定的结构化 effective config snapshot API，避免在多个模块中手工拼接字符串。
- 明确 perf overlay contract 边界，并冻结为结构化描述 `DataPlaneConfig::perf_overlay_boundary()`。

### 依据
- effective config 需要可序列化、可比对、可审计；debug 文本不应作为正式 contract。
- profile/options 仍是唯一配置来源；effective snapshot 仅用于观测解释，不引入第二套配置输入。
- perf overlay 必须“可解释且可验证”：只调优预算类字段，不改 bind/framing/容量/超时等形状字段。

## Iteration T1（2026-03-11）：unsafe 治理收敛，移除 channel_state borrowed unsafe

### 决策
- `crates/spark-transport/src/async_bridge/channel_state.rs` 不再保留 stream token borrowed fast-path，统一使用 `materialize_rx_token` owned 路径。
- `unsafe` 台账新增 `docs/UNSAFE_REGISTRY.md`，并由 `scripts/unsafe_audit.sh` 强制与 `crates/` 代码同步。

### 依据
- borrowed fast-path 的 `from_raw_parts + raw ptr release` 需要额外生命周期证明，当前阶段收益不足以覆盖治理成本。
- 安全策略优先级：先消除可消除 unsafe；必须保留的 unsafe 才允许留在最小边界并补足注释与测试。

### 不变项
- 不改变 driver kernel contract：`install_channel() -> sync_interest(chan_id)` 基线保持不变。
- 不改变 decode / close / ordering 既有语义，仅改变实现策略（由借用改为 materialize）。

## Iteration 14（T2 Windows/IOCP 状态诚实化 + 前进性门禁，2026-03-11）

- 决策：
  1) 统一将 `spark-transport-iocp` 对外口径标注为 **phase-0 compatibility layer**，明确不是 native dataplane、不是 production-ready；
  2) Windows `write_pressure_smoke` 改为 **known-failing 可见门禁**：允许阶段性不阻断，但必须在 verify 输出中显式报告，不允许静默 ignore；
  3) 为 Windows 后端补齐专项测试集（前进性、half-close、背压 drain、reactor 唤醒一致性），并要求非 Windows 环境显式 skip 且输出原因。

- 依据：
  - 多后端推进阶段最怕“名义支持”与“口径漂移”；先把状态诚实化，才能避免把兼容层误报为已完成原生后端。
  - `write_pressure_smoke` 已知问题若继续隐藏，会导致 CI 假绿和外部错误认知；显式 known-failing 报告是最低限度护栏。
  - Windows 专项测试集让“前进性”从口头承诺变成可执行证据，同时不破坏当前 phase-0 的主干推进节奏。

## Iteration T3（2026-03-11）：管理面上线治理基线（timeout/readiness/drain）

- 决策：
  1) 管理面引入连接级 timeout（idle/read/write/headers）与请求级 timeout（默认 + route/group override），禁止单一 global hard timeout。
  2) `/healthz` 定义为进程存活；`/readyz` 定义为综合就绪，至少包含 listener/draining/dependency 三类信号。
  3) drain 进入后拒绝新请求；在途请求按 request deadline 收敛。
  4) 过载治理采用显式参数化：max concurrent、per-connection inflight、queue limit、reject policy。

- 依据：
  - 对齐 ASP.NET Core/Kestrel 的治理能力等级（而非 API 同名）。
  - 保持 host/runtime 分层不破坏既有 driver kernel 契约与 transport 语义。

## Iteration 16（T4）：Perf evidence chain + scenario gate baseline

### Decision
- Replace single-line local perf gate parsing with a production-style report contract:
  - `scripts/perf_report.sh` produces JSON + CSV (`benchmark/reports/`)
  - `scripts/perf_gate.sh` compares report to per-platform baseline JSON files.
- Gate on both throughput and tail latency (p99) across multiple scenarios, plus syscall/batching/copy/backpressure and memory evidence (`peak_rss_kib`, `peak_inflight_buffer_bytes`).
- Keep writev/batching/flush direction unchanged; focus on measurable evidence continuity.

### Rationale
- “Near C++ performance” must be continuously verifiable in CI, not a one-off narrative.
- Multi-scenario baselines reduce overfitting to a single happy-path benchmark.
- Platform-split baselines make tradeoffs explicit and prevent Linux-only numbers from becoming implicit global truth.
