# “星火” (Spark) 高性能网络基础设施产品需求规格说明书

|**文档信息**|**内容**|
|---|---|
|**文档编号**|**SPARK-PRS-V4.6**|
|**项目名称**|Spark Framework ("星火")|
|**版本号**|**v4.6 (Physical Golden Master)**|
|**状态**|**已批准 (Approved)**|
|**密级**|**核心机密 (Core Secret)**|
|**发布日期**|**2026-01-31**|
|**版本摘要**|法律级工程契约最终封版：修复附录E物理排版闭合问题，固化Markdown渲染稳定性；全量包含Canonical Build ID、Embedded Event ID、Unsafe判定及0-RTT分级策略。|

---


## 当前工程状态快照（2026-03 / T2）

- Linux dataplane：`spark-transport-mio` 为当前 production baseline。
- Windows IOCP：`spark-transport-iocp` 当前为 **phase-0 compatibility layer**（wrapper），**not production-ready native dataplane**。
- Windows `write_pressure_smoke`：已知前进性问题，作为 known-failing 项显式报告（不允许静默 ignore）。
- 详见：`docs/GAP_STATUS.md`、`docs/KNOWN_ISSUES.md`、`docs/BACKEND_CONTRACT_MAP.md`。

---

# 1. 引言

## 1.1 编写目的

本文档是“星火”框架开发的唯一真理来源。它定义了该框架如何解决跨领域（高频交易、工业控制、军工、流媒体）通信中的系统性痛点，并提供一套统一的、高性能的、默认安全的 Rust 网络开发基础设施。

## 1.2 适用范围

本文档适用于 Spark 框架的所有核心组件（Core, Engine, Components, Embedded）的开发、测试与验收。

---

# 第一部分：问题全景：跨领域通信挑战的统一视图

本部分为“星火”框架的构建提供根本性理由。我们深入剖析贯穿不同行业（金融、工业、军工）的系统性挑战。

## 第一章：基础传输层的痛点

### 1.1. TCP：可靠性的双刃剑

TCP 协议是可靠通信的基石，但在特定专业领域中，其为了“通用性”而做出的设计成为了性能瓶颈。

- **连接开销**：三路握手引入了整整一个 RTT 延迟，对于高频交易（HFT）等对纳秒敏感的应用是不可接受的。
    
- **队头阻塞 (HoL)**：作为字节流协议，单个数据包的丢失会阻塞后续所有数据的交付，这对实时流媒体应用是致命缺陷。
    
- **拥塞控制的复杂性**：标准算法（Reno/CUBIC）在无线网络中常将信号干扰误判为拥塞，导致吞吐量不必要地下降。
    
- **状态管理地狱**：TIME_WAIT 状态在高并发下耗尽端口资源，导致服务不可重启。
    
- **Spark 解决方案**：框架必须支持 **Kernel Bypass (DPDK/AF_XDP)** 集成，并提供深度可配置的 TCP 组件（支持拥塞算法替换、缓冲区精细控制、Linger 策略）。
    

### 1.2. UDP：应用层可靠性的重负

UDP 提供了速度，但将可靠性责任全部推给了应用层，导致开发复杂度剧增。

- **应用层复杂性**：开发者被迫重新实现分片、重传、保序逻辑，极易出错且难以调试。
    
- **NAT 穿越难题**：无状态特性导致 NAT 映射极易超时，需实现复杂的心跳保活机制（STUN/ICE）。
    
- **安全隐患**：易成为 DDoS 放大攻击的媒介；缺乏拥塞控制容易压垮网络。
    
- **Spark 解决方案**：框架必须提供模块化的 **R-UDP (Reliable UDP)** 组件，允许用户通过构建者模式按需开启可靠性特性（ACK、重传、拥塞窗口）。
    

### 1.3. 串口：连接物理世界的脆弱桥梁

工业场景的基础，但故障模式微妙，难以诊断。

- **配置参数失配**：波特率、校验位微小的差异即导致通信全毁，且无明显报错。
    
- **调试黑盒**：无法区分是线路噪音、驱动问题还是代码逻辑错误。
    
- **Spark 解决方案**：框架必须内置 **软件环回测试** 与 **配置快照转储** 功能，在报错时自动 Dump 当前物理层配置，实现可诊断性。
    

## 第二章：现代执行环境的挑战

### 2.1. WebAssembly：沙箱边界的高昂代价

Wasm 运行在严格沙箱中，与宿主环境（Host）的交互成本极高。

- **JS 互操作开销**：每一次跨边界调用都伴随显著的性能损耗，I/O 密集型应用深受其害。
    
- **Spark 解决方案**：框架必须提供基于 **共享内存 (SharedArrayBuffer)** 的零拷贝桥接机制，最小化 Host-Guest 穿梭开销。
    

### 2.2. 嵌入式系统：约束的暴政

嵌入式开发遵循极其严苛的规则，标准库不可用，动态内存受限。

- **no_std 环境**：代码必须基于 Rust `core` 库编写，无法使用 OS 提供的线程与网络堆栈。
    
- **严格的内存管理**：动态分配（Heap Alloc）因碎片化风险和不确定性，在安全关键系统中被严禁。
    
- **Spark 解决方案**：框架必须支持 **稳态零堆内存 (Steady-State Zero-Heap)** 模式，利用 `const generics` 和静态内存池替代动态分配。
    

## 第三章：高风险领域的特定需求

### 3.1. 金融 (HFT)：对零延迟的追求

- **核心需求**：端到端延迟 **< 1µs**，且必须消除抖动（Jitter）。
    
- **技术映射**：架构必须支持 **Thread-Per-Core** 绑定以消除上下文切换，并支持 **无锁数据结构** (Ring Buffer)。
    

### 3.2. 工业 (ICS/SCADA)：确定性与安全的至高无上

- **核心需求**：硬实时响应（**< 1ms 截止时间**），必须兼容老旧不安全协议（Modbus/DNP3）。
    
- **技术映射**：嵌入式调度器必须支持 **RTIC (基于中断的抢占式调度)**，而非协作式异步。
    

### 3.3. 军事 (MIL)：绝对安全与韧性

- **核心需求**：默认全加密，防篡改审计，抗网络分区能力。
    
- **技术映射**：框架必须实行 **Default Deny** 策略，并内置不可变哈希链审计日志。
    

### 3.4. 流媒体：用户体验质量 (QoE)

- **核心需求**：秒开（VST < 2s），低缓冲率。
    
- **技术映射**：必须提供高效的管道架构以支持复杂的转码与分发逻辑，并集成 QUIC/SRT 支持。
    

## 第四章：跨领域的运维与安全噩梦

### 4.1. 分布式系统：驯服复杂性

- **常见痛点**：状态不一致、雪崩效应、配置地狱。
    
- **Spark 解决方案**：必须内置 **SLO 框架**、**统一背压 (Backpressure)** 和 **动态配置重载**。
    

### 4.2. 安全审计：合规性的不眠之眼

- **常见痛点**：缺乏证据链，无法满足 PCI-DSS/GDPR 审计要求。
    
- **Spark 解决方案**：必须内置 **哈希链审计日志** 和 **策略即代码 (OPA)** 引擎。
    

---

# 第二部分：“星火”框架：愿景与功能规范

## 第五章：指导原则

**5.1. 默认高性能，专家可调优**

框架提供适用于 90% 用例的高性能默认配置。对于 10% 的极端场景（如 HFT），通过 Traits 暴露底层参数（调度器、缓冲区、拥塞控制）供专家调优。

**5.2. 基于 Profile 的全景安全策略**

安全策略由 **Profile（场景配置文件）** 驱动：

- **General** (通用默认): 开启 mTLS，开启标准审计。
    
- **HFT** (高频交易): 允许在物理隔离前提下，使用轻量级认证，审计异步化（隔离模式）。
    
- **ICS** (工业控制): 兼容老旧协议，审计阻断（仅控制操作）。
    
- **MIL** (军工): 强制全加密，审计失败即阻断（严格模式）。
    
- **Streaming** (流媒体): 允许 QUIC 0-RTT，审计异步化。
    

**5.3. 物理隔离的统一 API**

**`spark-core`** 作为统一的 **“立法层” (Contract)**，仅包含 Trait 定义、Context、Error 类型与 Buffer 抽象，严格保持无 IO 运行时依赖。

**`spark-engine`** 作为 **“执法层” (Runtime)**，负责具体 IO 模型（Tokio/Uring）的实现与调度。

**5.4. 装配时与运行时的严格二分**

系统生命周期被物理切割为两个阶段：

- **装配时 (Assembly-time)**：可变 (Mutable)、富逻辑、允许 Panic、允许 Heap Alloc。用于构建管道。
    
- **运行时 (Run-time)**：不可变 (Immutable)、零锁 (Lock-Free)、零分配 (稳态数据面)、零 Panic。用于执行业务。
    

**5.5. 分层 Unsafe 治理**

- **L0 (`spark-core`)**: **Forbid Unsafe**。作为纯契约层，严禁包含任何 Unsafe 代码，**且严禁依赖任何包含 Unsafe 代码的 Crate** (无论直接或间接，判定口径见附录 I)。
    
- **L1 (`spark-engine`)**: **Deny Unsafe**。运行时实现层，仅允许通过独立审计的子模块引入必要的 FFI/系统调用。
    
- **L2 (`spark-components`)**: 允许受控 Unsafe (用于 FFI/DMA/MMIO)，需封装在 Safe API 内。
    
- **Unsafe 依赖管理**：任何必要的 Unsafe 实现必须隔离在独立 Crate (如 `spark-platform-unsafe`) 中，并记录在证据包的 `provenance/build_info.json` 中供审计。
    
- **判定口径**：本规范中“包含 Unsafe 代码的 Crate”定义为：其发布源码（含 feature 组合下的编译单元）存在任意 `unsafe` 块/`unsafe fn`/`extern "C"` FFI 声明。**CI 必须产出扫描报告 (`provenance/unsafe_dependency_report.json`) 并作为 Gate，L0 树中 unsafe 计数必须为 0。**
    

**5.6. 矛盾处理与决策规则**

为避免多目标共存导致的平庸化，执行以下铁律：

1. **Profile 不做折中，只做分化**：每个 Profile 拥有不可侵犯指标 (Invariants)。
    
2. **统一的是 L0 契约**：`spark-core` 仅定义最小语义契约。
    
3. **数据面与控制面硬隔离**：HFT/ICS 场景下，控制面不得干扰数据面。
    
4. **以发行物族交付**：严禁以“用户自行组合 Feature”替代 Artifact 交付。
    

## 第六章：运维与安全的军火库：功能规范 (分级版)

### 6.1. 运维工程师 (SRE/DevOps) 军火库

|**ID**|**需求名称**|**优先级**|**适用 Profile**|**详细规范描述**|
|---|---|---|---|---|
|**OPS-01**|**结构化日志**|**P0**|All|输出 JSON 格式，自动注入 TraceID/RequestID，严格转义防止日志注入。**针对 Artifact::Embedded-Minimal，允许使用二进制结构化格式 (TLV/CBOR)，但必须保证字段与 `log_schema.json` 同构，且证据包必须包含离线解码器及版本映射证明。**|
|**OPS-02**|**标准指标暴露**|**P0**|**Gen/HFT/MIL/Stream**|内置 Prometheus 端点，暴露 Runtime (CPU/Mem/Gauges) 与 Transport 核心 KPI。**指标命名与聚合口径必须符合附录 I 的 metrics_dictionary.yaml 字典。**|
|**OPS-03**|高基数保护|P1|Gen/Stream|指标注册必须提供 Label 白名单，运行时非法 Label Value 自动聚合为 "other"。|
|**OPS-04**|分布式追踪|P1|Gen/Stream|自动注入和提取 W3C Trace Context，跨服务边界传播。|
|**OPS-05**|SLO 框架|P1|Gen|提供 API 定义延迟/可用性目标，运行时自动计算并暴露错误预算 (Error Budget)。|
|**OPS-06**|实时仪表盘|P2|Gen|内置轻量级 Web UI，可视化健康状况与核心拓扑。|
|**OPS-07**|告警集成|P2|Gen|支持 Webhook 集成 PagerDuty/Slack，支持阈值触发。|
|**OPS-08**|深度传输指标|P1|All|暴露 TCP 重传数、CWND 大小、RTT 及 UDP 抖动/丢包率。_(注：指标暴露范围以发行物启用的 Transport Family 为准)_|
|**OPS-09**|管道监控|P1|All|自动测量 Pipeline 各阶段 (Codec/Service) 的 P99 延迟及队列深度。|
|**OPS-10**|Wasm 指标|P2|Gen|追踪 Host-Guest 跨边界调用的频率、延迟与内存开销。|
|**OPS-11**|**统一配置 API**|**P0**|**Gen/HFT/MIL/Stream**|支持从 Env/File/Consul 分层加载配置，提供类型安全的解析接口。|
|**OPS-12**|动态重载|P1|Gen|支持运行时配置热加载 (Hot Reload)。**配置必须版本化 (Versioned)，并通过 RCU/ArcSwap 实现原子切换；新配置生效前必须完成校验 (Schema/Signature/Policy)，校验失败必须“失败保旧 (Keep-Last-Good)”。每次切换 (成功/失败) 必须生成可审计事件与指标证据。运行时若检测到新配置导致健康检查连续失败或关键 SLO 退化 (触发条件: rollback_window + cooldown, 详见附录 I 之 config/runtime_policies.yaml)，系统必须支持回滚到最近一次 Last-Good 配置，并产生含回滚原因与影响范围的可审计事件。**|
|**OPS-13**|特性开关|P1|Gen|内置 Feature Flags 系统，支持按流量百分比或用户 ID 灰度开启。|
|**OPS-14**|自动回滚触发|P1|Gen|健康检查连续失败时，对外暴露标准信号 (Status/Webhook)，包含失败原因与证据指针。|
|**OPS-15**|**健康检查**|**P0**|**Gen/HFT/MIL/Stream**|提供 `/healthz` 端点，准确反映应用自身及下游依赖 (DB/Cache) 的状态。**`/healthz`=Liveness, `/readyz`=Readiness; Draining 状态下 /readyz 必须返回 Not-Ready。**|
|**OPS-16**|**优雅停机**|**P0**|**Gen/HFT/MIL/Stream**|响应 SIGTERM (或等价停止信号) 必须按顺序执行：**①停止接收新请求 (Quiesce Ingress) → ②进入 Drain 等待 in-flight 完成 → ③Flush 控制面证据 (审计/快照/DrainingEvent) → ④超时强制退出**。对支持协议层 draining 的传输族，必须发送可观测的“协议层 draining 信号”：<br><br>  <br><br>(1) 对支持流/会话语义的协议 (如 HTTP/2, QUIC)，必须发送显式拒绝新流/新会话的信号。<br><br>  <br><br>(2) 对不支持者 (如纯 TCP)，必须通过可观测方式对外宣告 Draining 状态并停止接收新连接。<br><br>  <br><br>(3) 具体信号按 Transport Family 定义在附录 I 的 transport/equivalence_draining.yaml 并在证据包中体现。|
|**OPS-17**|依赖就绪探针|P1|Gen|上游关键依赖就绪前，应用不标记为 Healthy，防止启动时流量黑洞。|
|**OPS-18**|**统一背压**|**P0**|All|**必须实现可组合且可证明的背压状态机：触发 (High Watermark：队列水位/预算耗尽) → 传播 (统一的 Pause-Read 或显式拒绝信号；Artifact 内全局一致) → 恢复 (Low Watermark：Drain 到阈值以下)。**<br><br>  <br><br>**约束：**<br><br>  <br><br>1. Low Watermark 必须严格小于 High Watermark，并采用滞回 (Hysteresis) 避免频繁抖动。<br><br>  <br><br>2. 背压计量单位在同一 Artifact 内必须唯一且固定 (Bytes/Msg/Unit)。<br><br>  <br><br>3. **可观测证据**：状态变更必须产生包含触发原因、水位值、计量单位及关联连接/通道 ID (Scope+ID) 的事件 (BackpressureEnter/Exit)，并暴露当前状态指标。<br><br>  <br><br>4. Pending 仅作为 Pause-Read 内部细节，对外验收以“水位线触发/恢复的可观测事件与指标”为准。<br><br>  <br><br>5. **传播动作**：按 Transport Family 定义最小等价动作集合 (详见附录 I 之 transport/equivalence_backpressure.yaml)，同一 Artifact 内必须一致。针对 `Artifact::Embedded-Minimal`，传播动作必须包括禁用中断、静态队列拒绝等，且 **`scope` 必须取值于集合 `{ "rtic_task", "isr", "channel" }` 并携带 `scope_id`。**<br><br>  <br><br>6. **单位映射**：事件必须包含 unit_mapping 字段以证明 Budget 单位与背压单位的映射关系 (映射定义必须可复算或引用版本，详见附录 H)。|
|**OPS-19**|负载削减|P1|Gen/Stream|内置令牌桶及自适应熔断器，过载时优先丢弃低优先级请求。|
|**OPS-20**|环境基线器|P2|Gen|启动时校验 OS 参数 (sysctl/ulimit) 是否符合最佳实践，并输出告警。|
|**OPS-21**|按需剖析|P2|Gen|运行时开启 CPU/Memory Profiling HTTP 端点 (需高权凭证激活)。|
|**OPS-22**|请求级调试|P2|Gen|支持通过特定 Header 开启单次请求的 Debug 级别日志。|
|**OPS-23**|应急预案链接|P1|Gen|告警信息中自动包含对应的 Runbook URL。|
|**OPS-24**|故障快照|P1|All|发生 Panic 或严重 Error 时，自动 Dump 栈信息、配置快照与核心指标。**约束**：<br><br>  <br><br>1. **Hook 策略**: `panic=unwind` 时 In-Process Hook 必须产出快照；`panic=abort` 时 In-Process Hook 不作为证据要求。<br><br>  <br><br>2. 快照必须预分配 (或控制面隔离线程执行)，确保不破坏 HFT/MIL 不变量。<br><br>  <br><br>3. **对 `panic=abort` 的 Artifact (HFT/MIL)，最小证据集由 Crash Sentinel (独立守护进程/宿主 Watchdog/RTIC 异常处理器) 负责产出并归档，确保证据链不断裂。**|
|**OPS-25**|混沌钩子|P2|Gen (Test)|内置故障注入能力 (延迟/错误)，仅在非生产构建 (`chaos` feature) 中可用。|

### 6.2. 安全与合规官 (Security) 军火库

|**ID**|**需求名称**|**优先级**|**适用 Profile**|**详细规范描述**|
|---|---|---|---|---|
|**SEC-01**|**默认 mTLS**|**P0**|**All**|默认开启；HFT 豁免需满足：限定 `Artifact::HFT-Bypass`、显式配置、启动即审计、证据包包含物理隔离指针。|
|**SEC-02**|策略即代码|P1|Gen/MIL|集成 OPA/Rego 引擎，策略加载失败默认为“拒绝所有”。|
|**SEC-03**|**最小权限**|**P0**|All|API 路由默认 Deny，需显式声明 Allow 规则。|
|**SEC-04**|WASM 权限模型|P1|Gen|加载 Wasm 模块前强制声明 WASI 能力 (FS/Net)，默认沙箱隔离。|
|**SEC-05**|安全启动集成|P1|ICS/MIL|嵌入式版本支持与硬件信任根交互，验证固件签名及防回滚版本。|
|**SEC-06**|**0-RTT 策略**|**P0**|**Gen/HFT/Stream (Support) + MIL/ICS (Hard Disable)**|**MIL/ICS 永久硬禁用 (Hard Disable)。** HFT/Stream 开启必须基于 **静态 Allowlist (位于证据包 security/allowlist/early_data_allowlist.yaml)**，未在清单内的请求必须拒绝进入 Early Data。**若 Allowlist 缺失或校验失败，必须执行 Fail-Closed (拒绝所有 Early Data) 并产生审计事件 (SEC-13)。** 清单变更触发 SEC-13 审计。|
|**SEC-07**|证书吊销检查|P1|**Gen/HFT/MIL/Stream**|优先 OCSP Stapling。MIL: 检查失败即阻断；HFT/Stream: 降级报警 (Fail-Open w/ Alarm)。|
|**SEC-08**|**统一密钥管理**|**P0**|All|抽象 KMS/Vault 接口，API 仅返回不透明 Handle，内存中无明文私钥常驻。|
|**SEC-09**|自动轮换|P1|Gen/MIL|支持凭证无重启自动轮换，不中断现有长连接。|
|**SEC-10**|DLP 钩子|P1|Gen/MIL|出站管道预留数据防泄漏扫描接口 (Hook)。|
|**SEC-11**|**不可变审计**|**P0**|All|审计日志使用哈希链 (Hash Chain) 链接，支持 TSA 签名。|
|**SEC-12**|**审计失败处理**|**P1**|**All**|MIL: 阻断业务；ICS: 仅阻断控制操作，硬实时循环走 Local Seal (封存成功为门槛)；HFT/Stream/Gen: Local Seal 模式，恢复后补录。|
|**SEC-13**|全面审计事件|P0|All|覆盖认证、授权、配置变更、密钥访问、策略变更等关键事件。|
|**SEC-14**|标准日志模式|P1|Gen/MIL|审计日志符合 CEF/LEEF 标准，便于 SIEM 集成。|
|**SEC-15**|**主体关联**|**P0**|All|**所有审计事件必须关联 Subject**。字段：`subject_type` (user/device/service/anonymous), `subject_id`。当 ID 不可得时 (如认证前)，必须为 JSON null，但**必须**携带 `conn_id`/`channel_id` 等关联键。|
|**SEC-16**|**供应链门禁**|**P0**|All|CI 流程强制执行 `cargo vet` (审查) 和 `cargo deny` (漏洞/协议)。|
|**SEC-17**|输入净化库|P1|Gen|提供防注入 (SQLi/XSS/Command) 工具库。|
|**SEC-18**|**Codec 安全**|**P0**|All|强制解析消耗 Budget。Budget 单位必须与 OPS-18 背压单位一致或可映射。Budget 耗尽触发 BudgetExceeded 事件与背压 (不可降级)。|
|**SEC-19**|TCP 加固|P1|**Gen/HFT/MIL/Stream**|监听器默认开启 SYN Cookie。DPDK/用户态栈需提供等价防护能力 (详见附录 I之transport_hardening_equivalence.md)。|
|**SEC-20**|SBOM 生成|P1|All|构建时自动生成 SPDX/CycloneDX 格式 SBOM 并签名。|
|**SEC-21**|合规配置包|P2|Gen|预置 PCI-DSS / GDPR / IEC 62443 合规配置模板。|
|**SEC-22**|数据驻留|P2|Gen|支持基于地理区域 (GeoIP) 的数据处理与路由控制。|
|**SEC-23**|渗透测试模式|P2|Gen|需多因素/高权凭证激活，开启详细诊断，并触发全量审计。|
|**SEC-24**|ASVS 对齐|P2|Gen|HTTP 管理面组件默认配置符合 OWASP ASVS Level 2 标准。|
|**SEC-25**|时钟中毒防护|P2|All|监控系统时钟偏移，关键操作 (TTL/Lease) 强制使用单调时钟 (monotonic_ts) 并同时记录墙钟 (wallclock_ts)。|

---

# 第三部分：“星火”框架的架构蓝图

## 第七章：核心运行时与调度器

### 7.1. Executor 能力分级与双契约

为解决嵌入式与服务器端执行模型的本质冲突，`spark-core` 定义双重契约，严禁混用：

- **`trait AsyncExecutor` (Server Contract)**: 定义 `spawn` / `spawn_local`。仅适用于依赖 `spark-engine` 的 Artifact。
    
- **`trait RtExecutor` (Embedded Contract)**: 定义 `post` / `dispatch` (RTIC 语义)。仅适用于 `Artifact::Embedded-Minimal`。**严禁 Embedded 依赖 AsyncExecutor。**
    

### 7.2. 服务器端策略 (Server Profile - implemented in `spark-engine`)

- **模型**: Thread-Per-Core。
    
- **IO 驱动**: 优先 `io_uring`，降级 `epoll` (封装于 `spark-engine`)。
    
- **隔离**: 控制面运行在低优先级线程；HFT-Bypass 必须运行在物理隔离核心 (Isolated Core)。
    
- **机制**: 每个连接在装配期被分配给一个核心，运行期永久绑定，消除跨核同步。
    

### 7.3. 嵌入式端策略 (Embedded Profile - implemented in `spark-embedded`)

- **模型**: RTIC (Real-Time Interrupt-driven Concurrency)。
    
- **IO 驱动**: 硬件中断 (NVIC)。
    
- **依赖**: 仅依赖 `spark-core`，**严禁引入 `spark-engine` 或依赖 AsyncExecutor**。
    

## 第八章：双阶段管道架构

Spark 采用**双阶段管道 (Two-Stage Pipeline)** 架构，并物理强制装配时与运行时的隔离。

### 8.1. 第一阶段：传输管道 (Transport Pipeline)

- **组件**: `InboundCodec` / `OutboundCodec` (位于 `spark-core`)。
    
- **职责**: 处理 **byte stream (u8 序列, 不指代 bytes crate)**。负责分帧、TLS 加解密、序列化。
    
- **物理约束**: **无外部副作用 (No External Side Effects)**。
    
    - **严禁**: IO 操作、访问全局可变状态、访问时间源/随机源（除非显式注入，且可替换/可测试）。
        
    - **允许**: 维持局部、连接私有的协议状态（如 TLS Session），以状态机形式产生 I/O 意图 (IO Intent, 详见附录 B)。
        

### 8.2. 第二阶段：应用管道 (Application Pipeline)

- **组件**: `Service` / `Layer` (位于 `spark-core`)。
    
- **职责**: 处理 **结构化消息 (Message)**。
    
- **必须契约**：`Codec/Service` 在可中断点 (每次 Poll, 长循环步进, 预算扣减) 必须检查 `Context` 中的取消/截止时间，并返回可判定的 `Cancelled/DeadlineExceeded` (优先级: Deadline > Cancel)；不得吞并或降级为通用错误。
    

### 8.3. 架构灵魂：装配时 vs 运行时

- **装配时 (Assembly-time)**: 可变 (Mutable)、允许 Panic、允许 Heap Alloc。
    
- **运行时 (Run-time)**: 不可变 (Immutable)、零锁 (Lock-Free)、零分配 (稳态数据面)、零 Panic。
    
- **约束**:
    
    - 消息对象 (Message) 允许借用底层 Buffer (Zero-Copy Borrowing)。
        
    - 当 `Service` 首次返回 `Pending` (发生 await/yield) 之前，若输入为 Borrowed Message，则必须在该点之前完成 Owned Materialize (Copy/Refcount Slice 固化)。
        
    - `Artifact::HFT-Bypass` 与 `Artifact::MIL-Strict` 的运行时产物必须使用 `panic=abort`；lint/静态检查仅作为附加证据。
        

## 第九章：组件与 Crate 设计：物理隔离的生态系统

为了实现“按需付费”并解决嵌入式依赖问题，代码库执行 L0-L3 物理分层。**本章定义唯一权威 Crate 拓扑。**

- **L0: 契约核心 (`spark-core`)**
    
    - **特性**: `#![no_std]` 兼容 (通过 feature 切换)，`#![forbid(unsafe_code)]`。
        
    - **职责**: 定义生态宪法。
        
    - **Buffer 抽象 (双重契约)**: 定义纯 Trait `Buffer`，**不依赖 `bytes` crate (含 unsafe)**。
        
        - **Embedded 实现**: 由用户或 L3 提供基于 `&'static [u8]` 的 No-Alloc 实现。
            
    - **依赖约束**: 严禁依赖 Tokio, Async-std，**严禁依赖任何包含 Unsafe 代码的 Crate** (无论直接或间接)。
        
- **L1: 运行时引擎 (`spark-engine`)**
    
    - **特性**: `std` 环境，依赖 `tokio` (或全自研 Reactor)。
        
    - **职责**: 驱动契约执行，实现 `AsyncTransport`，管理 Thread-Per-Core 调度。
        
    - **Unsafe**: `#![deny(unsafe_code)]`，仅允许审计过的 FFI。
        
- **L2: 积木组件 (`spark-components`)**
    
    - **职责**: 提供具体功能的实现。
        
    - **子族**:
        
        - `spark-transport-*`: TCP, UDP, UDS 实现。
            
        - `spark-security-*`: TLS, OPA 适配。
            
        - `spark-buffer-bytes`: **(新增)** 适配 `bytes::Bytes` 为 L0 Buffer 接口，引入 Unsafe 依赖，仅供 Server 端使用。
            
    - **约束**: 接口依赖 `spark-core`，实现可依赖 `spark-engine` (若为 Server 组件)。
        
- **L3: 集成层 (`spark-embedded` / `spark-server`)**
    
    - **内容**:
        
        - `spark-embedded`: 适配 RTIC，仅依赖 `spark-core`。
            
        - `spark-server`: 聚合 `spark-engine`, `spark-components`，提供开箱即用体验。
            

## 第十章：API 设计与 Rust 惯用法

### 10.1. 类型状态构建器 (Type-State Builder)

利用 Rust 泛型系统，将配置错误拦截在**编译期**。

- **规范**: `ServerBuilder` (位于 `spark-engine`) 初始状态为 `NoTls`。`listen()` 方法仅对 `ServerBuilder<TlsConfigured>` 可见。
    

### 10.2. 零样板代码

提供 `pipeline!` 宏，以声明式 DSL 组装 `spark-core::Service` 和 `spark-core::Layer`。

---

# 第四部分：验证与验收目标

## 第十一章：参考实现验证

需交付以下参考实现以验证架构能力：

1. **SIP Server (Gen Profile)**：验证文本/二进制混合解析 (Codec)、复杂状态机 (Service)。基于 `spark-engine`。
    
2. **RTSP Server (Stream Profile)**：验证多流 (TCP/UDP) 协调。基于 `spark-engine`。
    
3. **Modbus Adapter (ICS Profile)**：验证硬实时、零堆内存。**基于 `spark-embedded` + `spark-core`，不依赖 `spark-engine`，验证 L0 Buffer 的 No-Alloc 实现。**
    

## 第十二章：性能基准与验收方法学

本章定义不可协商的性能验收标准。所有测试必须遵循 **附录 D** 的方法学模板，并严格绑定 **Artifact + Profile + 工况**。

### 12.1. HFT 场景 (<1µs Latency)

- **适用 Artifact**: `Artifact::HFT-Bypass`
    
- **Profile**: `Profile::HFT` (审计隔离)
    
- **承诺范围**：仅对 `DataPlaneFastPath` 承诺。
    
- **工况 A (Kernel Bypass, 双机直连)**: P99 < 5 µs (跨机直连), Jitter < 200 ns。
    
- **工况 B (High Perf IO, Loopback)**: P99 < 1 µs (同机双端口物理回环), Jitter < 200 ns。
    
- **测量口径**: 附录 D 报告必须包含：测量点位置 (Measurement Points)、时钟源类型 (Timestamp Source)、CPU 频率锁定方法、样本数据哈希。
    

### 12.2. ICS 场景 (硬实时)

- **适用 Artifact**: `Artifact::Embedded-Minimal`
    
- **验收工况**: Cortex-M7, 100% CPU 背景负载 (Dhrystone), RTIC 调度。
    
- **目标**: 中断响应延迟 **< 10 µs**, 任务截止时间违约率 **0%**。
    

### 12.3. 高并发场景 (>1M Conn)

- **适用 Artifact**: `Artifact::General-Server`
    
- **验收工况**: Keep-Alive 连接 (90% Idle), Low-Cardinality Metrics (全局聚合).
    
- **目标**: 单机 100 万连接, 内存 < 20GB, 新建速率 > 50k/s.
    

### 12.4. 嵌入式资源约束

- **适用 Artifact**: `Artifact::Embedded-Minimal`
    
- **验收工况**: STM32F4, no_std, StaticBufferPool, Strip Symbols.
    
- **目标**: 二进制体积 **< 20 KiB** (Stripped), 堆内存使用 **0 Bytes** (全生命周期)。**验收必须基于 L0 Buffer 的 No-Alloc 实现。**
    

## 第十三章：安全与质量保证

### 13.1. 验证方法论

- **Fuzzing**: `spark-codec` 必须在 CI 中通过 10 分钟持续 Fuzzing (honggfuzz)。
    
- **Contract Test**: `spark-core` 提供测试套件，通过 Allocator Hook 验证稳态零分配。
    
- **Chaos Engineering**: 注入时钟跳变、网络分区，验证系统自愈能力。
    
- **Build Provenance (证据化构建元信息)**：CI 必须生成并归档关键产物的构建参数与元信息证据。**最小证据字段清单**：版本号/Commit Hash、目标三元组 (Target Triple)、Rustc 版本、Panic 策略、Opt-Level、LTO 设置、Codegen-Units、链接器参数、启用的 Feature 集、SBOM 指纹/签名指纹、构建时间与环境标识、**unsafe_crates 列表及审计引用**。
    

### 13.2. 合规交付物

- **审计完整性**: 自动化脚本验证哈希链连续性及 TSA 签名。
    
- **供应链门禁**: CI 强制 `cargo vet` & `cargo deny`。
    
- **证据包**: 构建系统自动生成 SBOM (SPDX/CycloneDX) 及合规性映射报告。**证据包必须包含 CI 生成的 Traceability Matrix (`test_results/traceability_matrix.csv`)。**
    

---

# 附录 A: Spark Profile 全景矩阵 (Capability Matrix)

|**特性维度**|**General (默认)**|**HFT (高频交易)**|**ICS (工业控制)**|**MIL (军工)**|**Streaming (流媒体)**|
|---|---|---|---|---|---|
|**mTLS**|开启 (P0)|**豁免 (物理隔离 + 显式豁免 + 审计事件)**|开启 (兼容老旧)|**强制** (P0)|开启|
|**0-RTT**|禁用 (P0)|允许 (P1, QUIC)|**禁用 (Hard Disabled)**|**禁用 (Hard Disabled)**|允许 (QUIC)|
|**审计模式**|标准 (Async)|**隔离** (Local Seal)|标准|**严格** (Block)|标准|
|**审计阻断**|否|**否**|是 (仅控制面)|**是**|否|
|**吊销检查**|Stapling/Cache|Stapling/Cache|忽略 (内网)|**Hard-Fail**|Stapling|
|**调度模型**|Thread-Per-Core|Thread-Per-Core|**RTIC**|Thread-Per-Core|Thread-Per-Core|
|**Executor Contract**|**Async**|**Async**|**RTIC (No-Async)**|**Async**|**Async**|
|**Unsafe 构建策略**|**Policy-GEN**|**Policy-HFT**|**Policy-ICS**|**Policy-MIL**|**Policy-STREAM**|
|**内存模型**|Jemalloc|**Hugepages**|**StaticPool**|Jemalloc|Jemalloc|

> **注：Unsafe 构建策略说明**
> 
> - **Policy-GEN**: 遵循 5.5，默认不引入 `spark-platform-unsafe`。
>     
> - **Policy-HFT**: 允许引入 `spark-platform-unsafe`，需审计。
>     
> - **Policy-ICS**: 默认不引入；仅在硬件适配确需时允许，必须审计。
>     
> - **Policy-MIL**: 仅允许经审计的必要 Unsafe，强制证据包。
>     
> - **Policy-STREAM**: 遵循 Policy-HFT 审计基准。
>     

---

# 附录 B: 术语表 (Glossary)

- **Spark Core**: 核心契约 Crate，不包含 IO 运行时实现。
    
- **Spark Engine**: 运行时 Crate，包含调度器和 Reactor。
    
- **Spark Components**: L2 组件 Crate 族。
    
- **Steady-State Zero-Alloc**: 指系统完成启动预热后，处理单个请求的数据面路径（Codec+Service）不再触发系统级堆分配（malloc/free）。
    
- **Lock-Free (Data Plane)**: 指数据面路径不包含 `Mutex` 或 `RwLock`，仅允许原子操作或 RCU。
    
- **Local Seal Mode (审计)**: 当审计服务不可用时，将日志写入本地固定大小的环形缓冲区或只追加文件，并计算哈希链。
    
- **0-RTT**: 特指 TLS 1.3 Early Data 或 QUIC 0-RTT 握手特性。
    
- **Backpressure State Machine**: 触发条件(队列水位/预算耗尽) -> 传播(暂停 Read/返回 Pending) -> 恢复(Drain)。
    
- **Zero-Copy Boundary**: Message 允许借用 Buffer；跨 await/Core 必须 Copy。
    
- **IO Intent**: Codec 允许产生 `NeedRead/NeedWrite/HandshakeStep` 等意图信号；Transport 驱动层据此执行实际 I/O。
    

---

# 附录 C: 需求-设计-测试 追踪矩阵 (Traceability Matrix)

_(矩阵需由 CI 生成并归档，此处为示例)_

|**需求 ID**|**需求描述**|**涉及 Crate**|**验收测试用例**|**交付证据**|
|---|---|---|---|---|
|**SEC-01**|默认 mTLS|`spark-security` (依赖 `spark-core` 契约)|`test_server_panic_without_tls`|单元测试报告|
|**OPS-18**|统一背压|`spark-core` (定义状态机) + `spark-engine` (执行暂停)|`test_pipeline_reject_overload`|压测报告 (P99 Latency)|
|**PERF-01**|<1µs 延迟|`spark-engine` (Thread-Per-Core 实现)|`bench_loopback_latency`|性能基准报告|
|**SEC-11**|审计哈希链|`spark-security`|`verify_audit_chain_integrity`|链验证脚本日志|
|**SEC-06**|0-RTT Allowlist|`spark-security`|`test_early_data_rejection`|审计事件日志|

---

# 附录 D: 性能验收方法学模板 (Performance Methodology)

所有性能指标必须基于此模板填写：

1. **测试拓扑**: (如：Client -> Switch -> Server)
    
2. **硬件规格**: (CPU 型号, 频率锁定状态, 网卡型号, 驱动版本)
    
3. **OS 调优**: (Hugepages, Isolcpus, IRQ Affinity, sysctl 参数)
    
4. **流量模型**: (消息大小, QPS, 并发数, 持续时间)
    
5. **统计方法**: (Warm-up 时间, 采样率, Outlier 处理策略)
    
6. **测量口径**: 测量点位置 (应用层/内核层/网卡层), 时钟源类型, CPU 频率锁定方法。
    
7. **结果输出**: (P50/P90/P99/P999, Jitter, 吞吐量, 样本数据哈希)
    

---

# 附录 E: 发行物族定义 (Artifact Families)

|**基础依赖**|**spark-core + spark-embedded**|**spark-core + spark-engine**|**spark-core + spark-engine (Bypass Feat)**|**spark-core + spark-engine**|**spark-core + spark-engine**|
|---|---|---|---|---|---|
|**不可侵犯指标 (Invariants)**|< 20KiB, 0 Heap, RTIC|mTLS, 可观测性, 稳态零分配|**P99 < 1 µs (工况 B), P99 < 5 µs (工况 A); Jitter < 200 ns**|Fail-Closed, Audit Block|VST < 2s, Loss Recovery|
|**核心特性集**|L0+L3, no_std|全功能集合|L0+L1+L2, Kernel Bypass|L0+L1+L2, Hardened Security|L0+L1+L2, QUIC/SRT|
|**禁止项**|OPA, Tracing, UI, Alloc|无|共享核竞争, 阻塞审计|0-RTT, Unsafe (非必要)|无|
|**Buffering 策略**|静态池 (Static)|默认内存缓冲 (不可落盘)|**禁止 Buffering (Zero-Buffer)**|默认内存缓冲|受控 Buffering (可配置落盘/RingBuf)|
|**典型工况**|STM32 / Cortex-M|K8s 微服务 / 网关|专用交易服务器|战术通信节点|边缘 CDN 节点|

> **注：Buffering/Spooling（含落盘）策略必须按 Artifact 明确分化**：
> 
> - `Artifact::HFT-Bypass`: 禁止任何形式的 buffering/spooling（包括落盘与跨阶段缓存扩张）。
>     
> - `Artifact::Streaming-Edge`: 可在显式配置下启用受控 buffering/spooling（需审计事件与配额上限）。
>     
> - `Artifact::General-Server`: 默认仅允许内存缓冲且不得自动落盘（除非显式配置并审计）。
>     

---

# 附录 H: 证据事件与指标字典 (Evidence Dictionary)

> **全事件通用必选字段**：`event_id` (全局唯一, Server Artifact 必须使用 UUIDv7; Embedded Artifact **必须** 使用 `provenance/event_id_scheme.json` 声明的生成方案，若 `wallclock_ts` 不可得则为 JSON null), `event_name`, `instance_id` (运行实例), `build_id` (SHA256 of RFC8785 Canonical JSON of `provenance/build_info.json`, **必须编码为小写十六进制字符串**, 长度固定 64), `config_version` (当前配置), `artifact`, `profile`, `wallclock_ts` (RFC3339), `monotonic_ts_ns`, `trace_id` (若不可用必须显式为 JSON null), `subject_type`, `subject_id` (可为 JSON null), 及至少一个关联键 (`conn_id`/`channel_id`/`scope_id`)。**Canonical JSON 的字节序列必须为 UTF-8 编码且不含 BOM；hash 输入为该 UTF-8 字节序列的原样 bytes（不额外附加换行）。**

|**事件名称**|**必选字段**|**触发条件**|**关联指标**|
|---|---|---|---|
|**BackpressureEnter**|scope(枚举值), scope_id, reason, high_watermark, unit, unit_mapping, propagation_mode|队列/预算达到高水位线|`backpressure_active{scope}`|
|**BackpressureExit**|scope(枚举值), scope_id, duration_ns, low_watermark, unit|Drain 至低水位线以下|`backpressure_total_duration`|
|**DrainingEnter**|reason(sigterm/config/manual), deadline, ingress_quiesced_at, protocol_signal_sent, readyz_state|接收到停止信号|`server_state{state="draining"}`|
|**DrainingExit**|duration_ns, reason(done/timeout)|Draining 完成或超时退出|`draining_duration_seconds`|
|**BudgetExceeded**|budget_before/after, unit, unit_mapping, parser_stage, payload_size, budget_limit, action_taken|Codec 解析预算耗尽|`codec_error_total{type="budget"}`|
|**Rollback**|window_config, slo_metric, baseline, degraded_value, cooldown_config, reason, selected_last_good_version|运行时回滚触发|`config_rollback_total`|
|**TlsExemptionGranted**|scope, reason, evidence_ptr (证据包路径), artifact|HFT 豁免 mTLS 启动|`tls_exemption_active`|
|**BufferingEnabled**|medium(mem/disk), quota, ttl, encryption, artifact, path_or_medium_id, config_version, cleanup_policy|启用 Spooling/Buffering|`buffer_usage_bytes`|

> **注：** `scope` 的取值集合必须由对应 Artifact 的等价字典 (`transport/equivalence_backpressure.yaml`) 与 OPS-18 约束共同决定；对 `Embedded-Minimal`，`scope` 必须取值于集合 `{ "rtic_task", "isr", "channel" }`。

---

# 附录 I: 证据包目录结构 (Evidence Pack Manifest)

**证据包顶层必须包含 `manifest.json`：列出每个文件的 SHA256、生成时间、工具版本及整包签名（或签名指纹）。**

1. **`provenance/`**:
    
    - `build_info.json`: 含版本、Commit、Rustc、Panic 策略、**unsafe_crates 列表及审计引用**、13.1 所有最小证据字段、buffer_adapter。
        
    - `crash_sentinel_info.json`: Sentinel 版本与策略 (针对 Abort 场景)。
        
    - `unsafe_dependency_report.json`: CI Unsafe 扫描报告与工具信息。
        
    - `event_id_scheme.json`: (仅 Embedded) 兼容生成器算法说明。
        
    - 签名文件 (.sig)。
        
2. **`sbom/`**: SPDX/CycloneDX 文件, 签名指纹。
    
3. **`audit/`**: 审计链校验报告, 链头哈希 (Chain Head Hash), `audit_local_seal_policy.md` (满策略说明)。
    
4. **`metrics/`**: `metrics_dictionary.yaml` (指标字典), 原始基准数据。
    
5. **`logs/`**:
    
    - `log_schema.json` (日志字段字典)。
        
    - **`embedded_log_decoder/`**: 二进制日志解码器及版本映射说明。
        
6. **`test_results/`**:
    
    - 性能测试脚本, 原始样本数据, 统计摘要 (Hash)。
        
    - 配置回滚验收脚本 (`config_rollout_testplan.md`)。
        
    - **`traceability_matrix.csv`** (CI 自动生成的需求-用例追踪矩阵)。
        
7. **`config/`**: 当前配置版本快照, 签名, 回滚事件链日志, `runtime_policies.yaml` (回滚策略参数; 其在证据包中的规范相对路径为 `config/runtime_policies.yaml`)。
    
8. **`security/`**:
    
    - `transport_hardening_equivalence.md` (等价防护清单)。
        
    - **`allowlist/`**: `early_data_allowlist.yaml` (0-RTT 静态名单) 及豁免名单。
        
9. **`transport/`**: `equivalence_draining.yaml` (Draining 等价信号字典), `equivalence_backpressure.yaml` (背压传播等价动作字典)。
    

---

# 附录 G: 审阅签署页

|**审阅结论**|**[x] Recommend Sign-off / [ ] Hold**|
|---|---|
|**审阅人**|联合专家组 / 首席架构师|
|**审阅日期**|2026-01-31|
|**备注**|**规格审阅结论：Recommend Sign-off (批准归档)。**<br><br>  <br><br>本版本 (v4.5) 已全量解决架构与工程契约的最后矛盾，并完成行政级物理勘误：<br><br>  <br><br>1. **架构自洽**：清除 spark-api 术语，统一 spark-components 命名，执行 L0 Buffer/Executor 双重契约分离。<br><br>  <br><br>2. **工程严谨**：引入 Canonical Build ID (RFC 8785) 与 Embedded Event ID 算法，自动化生成追踪矩阵。<br><br>  <br><br>3. **交付闭环**：Unsafe 判定证据化与 0-RTT Fail-Closed 约束，确保验收证据链绝对可复算、可判定。<br><br>  <br><br>4. **物理零争议**：修复 Markdown 渲染断裂，统一编码规范与边界条件。<br><br>  <br><br>**风险声明**：零新增豁免。|