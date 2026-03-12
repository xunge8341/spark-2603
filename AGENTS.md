# AGENTS.md — 仓库级 Codex 项目说明

## 项目定位
本仓库当前主线是 Rust 传输/宿主内核与治理能力建设；同时，业务目标对齐“跨安全边界业务交换网关”的生产约束。

在涉及跨网关业务描述时，请以以下边界为准：
- 控制面：SIP（小报文与状态回执）
- 文件面：RTP（大载荷，B -> A 单向）
- A 侧落地：受控 HTTP 模板执行（基于 `api_code`）

## 业务与协议约束（新增功能时必须遵守）
- SIP 使用 JSON Body 承载完整业务字段，Header 仅镜像少量索引字段。
- RTP 使用“二进制定长主头 + 可选 TLV 扩展段”。
- 签名首版采用 HMAC-SHA256；代码结构必须预留国密算法升级位（算法工厂/枚举扩展位）。
- 不实现任意 HTTP 透传；仅允许基于 `api_code` 的受控路由模板执行。
- 系统需具备：幂等、补片、重试、限流、审计、日志、指标、追踪、告警、运维后台。

## 工程红线
- 以当前工作区为唯一事实，不混用历史 zip 覆盖。
- 不修改 driver kernel 既有语义，不动 `install_channel() -> sync_interest(chan_id)` contract baseline。
- Rust 非测试代码保持 panic-free：禁止 `unwrap/expect/panic!/todo!/unimplemented!`。
- `unsafe` 仅允许在现有受控边界；新增 `unsafe` 必须附清晰 `Safety` 注释。
- 默认使用 `--locked`；若 `Cargo.lock` 变化，必须一并提交。
- 不为微优化引入跨层耦合、生命周期扩散或双轨配置。

## 代码风格
- 小函数、单一职责、命名直白。
- 不新增“临时过渡结构”作为长期设计。
- 注释只写边界与理由，不写重复代码行为。
- 公共 API 最小化：优先 `pub(crate)`，能不公开就不公开。
- 指标命名集中管理，不在业务逻辑中散落硬编码字符串。

## 文档同步要求
完成任何任务后，至少同步检查并按需更新：
- `docs/GAP_STATUS.md`
- 对应 workstream 文档
- 若设计边界变化，补 `docs/DECISION_LOG.md`
