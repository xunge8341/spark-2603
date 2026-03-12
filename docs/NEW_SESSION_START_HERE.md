# 新会话起步：RX 零拷贝 / 可控分配 / ASP.NET Core 体验

> 这份文档用于把当前全绿基线与下一阶段三大工作流一次性交接清楚，避免新会话误判。

## 当前全绿基线
- BigStep-28 Step-5 + contract 修正后全绿。
- Driver Scheduling Kernel 已完成地基收敛：Vocabulary/Single-enqueue/Task ownership/Interest edge-sync/metrics。
- `docs/DECISION_LOG.md` 末尾包含 Step-5 与 contract 修正的决策依据。

## 下一阶段的“唯一主线”（不要分散）
1) **RX 零拷贝闭环**：先消灭 token path 的双拷贝，再引入 owned-segment append。
2) **C++ 级可控分配**：先建立 alloc/realloc 证据，再引入 hard cap（默认兼容）。
3) **ASP.NET Core 体验落地**：profile/config 单一事实来源 + effective config 审计输出。

## 入口文档
- North Star：`docs/NORTH_STAR.md`
- 当前差距总览：`docs/GAP_STATUS.md`
- Driver kernel 已完成形态：`docs/DRIVER_SCHEDULING_KERNEL.md`

### 三大工作流详述
- RX 零拷贝：`docs/WORKSTREAM_RX_ZERO_COPY.md`
- 可控分配：`docs/WORKSTREAM_ALLOC_CONTROL.md`
- ASP.NET Core 体验：`docs/WORKSTREAM_ASPNET_CORE_EXPERIENCE.md`

## 关键工程注意事项（避免反复卡编译/门禁）
- 以“当前全绿工作区”为唯一事实基线；不要混用历史 zip 覆盖 + patch 叠加。
- `Cargo.lock` 与 `--locked`：一旦更新 lockfile，必须纳入提交；不要被 zip 覆盖回旧版本。
- Windows 上补丁文件若由 PowerShell 生成，注意避免 UTF-16 编码导致 `git apply` 报 garbage。
- 仓库级执行约束已固化到 `AGENTS.md`，包含跨安全边界业务交换网关的协议边界（SIP JSON body / RTP 定长头+TLV / `api_code` 受控路由）与工程红线；新任务开始前先对齐该文档。

## 建议的推进节奏
- BigStep-29：优先把 RX 双拷贝降为单拷贝（risk low, value high）。
- BigStep-30：在已有证据基础上推进“可控分配 hard cap”与 Host 配置 SSOT 收敛。
