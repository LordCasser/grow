# 模型控制与恢复专项审视

独立 reviewer：`magpie/gpt-6-luna`。现状调查基于工作树活动实现中的快照，不代表最终实现已验收；当前未运行测试。完整仓库与版本审计见独立 [audit change](../2026-11-05-audit-grow-architecture-across-versions/proposal.md)。

## A1 — 普通 resume 的来源选择所有者

现状：`agent/subagent/mod.rs::durable_resume_source_for` 已加载并验证 child Timeline，但 `resume_source_from_facts` 从父 Spawn 读取出生 model/transport/effort。child 启动 baseline 与后续 model change 由自身 Timeline 持有；来源运行中切换后再省略参数 resume 会返回出生配置。

对本 change 的影响：违背省略字段保留来源最后实际选择的契约。修复落在现有 durable source owner：canonical linkage 验证后复用 `session::persistence::latest_model_selection`，父 Spawn 只作为出生/生命周期事实。原身份 restore 继续读取 checkpoint frontier，不共用错误的出生值回退。

通过标准：来源先改变 model/effort（包含最终未设置 effort），结束后从真实 resume 入口省略或单字段覆盖，provider、派生 Spawn/child baseline 一致，来源 Timeline 不变。现有 checkpoint 测试不能替代普通 resume 生产链验证。该修复已交 resume coder，最终状态由后续实际 diff/测试复核。

## A2 — 接纳与应用必须分离

设计反例：ACP `agent/handlers/model_switch.rs::finish` 等待应用终态；`session/actor/model_switch.rs` 在活跃 Step 闭合后应用；`tool/mod.rs::execute_tool_calls` 要等待工具 future。当前 Step 内工具若等待 finish，会形成等待环。

这是新工具的验收约束，不将未完成接线记为已交付实现缺陷。工具必须等明确 actor admission ACK 返回 queued；不能只凭 channel.send 成功，也不能等待自身应用。Shell 保留后续应用 responder owner，按既有 catalog publication 顺序、latest-wins、durable route transition 和失败边界处理。

通过标准：真实工具调用在当前 Step 完成，下一 provider 请求使用新路由且原工具往返完整、执行一次；拒绝/关闭/持久化失败不冒充 applied；effort-only 与用户 pending model 正确合成。还需核对 host-bound child 安装和 Agent 重建，以及 Workflow/工具权限约束。

## 审查限制

Task 旧的 `resume_from_ignores_model_and_effort_overrides` 预期需随有意改变的契约更新。最终验收不能采用旧“非法 override 也成功”的预期。reviewer 没有编辑代码或运行检查；本记录由架构 owner 根据实际返回报告整理。最终候选需再次独立 review，并记录对应测试状态。

## 最终候选静态 review

同一独立 `magpie/gpt-6-luna` reviewer对实际diff和相关调用方再次核对，未确认实现阻塞。核实普通resume在完整canonical校验后采用child最后durable model selection（None effort也覆盖），逐字段patch、source transport pin、checkpoint frontier分流不放宽；新工具通过host session/incarnation绑定、共享catalog准备、独立actor admission ACK返回queued，后续finish由Shell持有；Workflow frozen route、Agent rebuild和child permit边界仍在调用链中。

一项P2验收证据缺口：provider级resume changed场景直接发送child actor的SetSessionModel，没有经过新工具入口。父架构owner已单独交resume coder在既有CLI mock-provider测试补“source child真实调用set_session_model切到无efforts模型→结束→省略字段resume”的单场景，验证最终effort=None、source工具交换一次、下一请求与派生Spawn/baseline一致且来源不变。测试运行状态以verification为准，不能将静态review当运行通过。

父侧统一新工具运行发现Hook拒绝fixture使用了无法由原生Agent parser识别的PreToolUse事件名。已形成独立测试夹具修复任务，要求使用实际native hook schema并证明Hook被安装/执行，不改生产schema或删除场景。其余当时新工具矩阵、Workflow和permit已通过；该结果仍是部分通过，待修正后最终定向复测。
