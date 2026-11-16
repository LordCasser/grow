# 根因与证据

调查基线：`733d2c72ed0436dc27f050ee2149dd41432be552`。开始时工作区干净；本轮只新增本 change 文档。以下为源码与现有测试的静态核对，未执行产品运行复现，不能把候选方案写成已经修复。

## 1. Behavior 已有持久化，缺少不依赖历史的恢复投影

`crates/codegen/shell/src/session/actor/session_mode.rs:905` 在切换前构造下一份 BehaviorSnapshot，经 `persist_behavior_transition_*_durably` 确认后才修改 live coordinator。`session/control.rs` 明确 Timeline Control 是唯一持久来源；`agent/mvp_agent/acp_agent.rs:888` 提取该快照，`session/actor/spawn.rs:1482` 校验并在 `:1502` 重建 BehaviorCoordinator。

实际缺口在客户端 hydration：

- `pager/src/app/session/mod.rs:1357` 将新 view 的 behavior 初始化为 Normal。
- `pager/src/app/acp_handler/session_notification.rs:2497` 只通过 CurrentModeUpdate 写入已确认 behavior 与 plan_phase。
- `shell/src/agent/mvp_agent/acp_agent.rs:1577` 的 load response 只有 config_options 与 metadata；`agent/session_config.rs:39` 只构建 model/effort，没有当前 Behavior 完整快照。
- `shell/src/session/actor/spawn.rs:3283` 只有 completed_plan 存在时补发模式；普通已恢复 Plan/Clarify/Workflow 不会统一补发。
- `AvailableCommandsUpdate` 虽有 behaviorAvailability.current，但不是 Pager 已确认 Behavior 的赋值通道，也不带完整 Plan phase。

因此：切换记录位于最近历史页以外时，后端可已恢复原 Behavior，界面仍停在 Normal。当前证据不能支持“所有 Behavior 都没有写盘”；应修复权威状态 hydration，并用进程级检查同时验证磁盘、actor、模型提示和 UI。

## 2. Plan 终端受阻来自额外硬门禁与提示词

`crates/codegen/shell/src/session/actor/tool/authorization.rs:612` 的 plan_mode_edit_gate 在未 Executing 时直接拒绝 `AccessKind::Bash` 和 Edit，另外限制 Task capability 与 MCP；`tool/preparation.rs:1006` 在普通权限链前调用。`session/behavior.rs:811` 把 plan_allows_edits 限定为 Executing；`actor/tool/mod.rs:2092` 的 bash_is_gated_during_drafting 测试固定了这项行为。

模型指令也禁止所有执行：`session/behavior.rs:1387` 的 Drafting/Amending 文案包含 “Workspace mutation and execution are forbidden”；`shell/prompts/behaviors/plan/base.md` 要求 Executing 才能修改工作区。只改终端授权会留下模型层面的冲突。

本次有意改变权限语义：Plan 与 Normal 使用同一普通工具权限链；Plan 的审批是行为协议，规划阶段每一次工具调用都须服务于计划研究，禁止把工具授权视为实施批准，也禁止借 shell/MCP/委派/Workflow 执行尚未批准的方案。审批身份、冻结 artifact 和越界修订继续由原 Plan owner 负责。

## 3. 历史加载停在独立 viewer，时间线只认识本地条目

已归档契约 `openspec/specs/client-surfaces/spec.md:5427` 要求最近页、按需旧页和稳定锚点；实现存在下列差异：

| 位置（均相对 crates/codegen/） | 当前行为 | 后果 |
| --- | --- | --- |
| pager/src/app/agent_view/input.rs:865 | 仅 Scrollback 焦点、PageUp 且 offset 已为 0 时请求旧页 | 滚轮、滚动条及其它上滚 action 不补历史 |
| pager/src/app/root/dispatch/task_result.rs:615 | 每页创建独立 TranscriptProjection | 页面没有共用完整的历史展示归属 |
| 同文件 :633 与 :639 | latest prepend 主 scrollback，older 赋给 history_page_viewer | 旧页不进主 view，下一页替换前一页 |
| pager/src/app/root/dispatch/tests/task_result.rs:13 | 断言 older viewer 存在且主 scrollback 长度不变 | 测试固定了与本次需求冲突的呈现方式 |
| pager/src/app/agent_view/render.rs:1240 | rail 总数、活动位置来自本地 scrollback | 未加载历史没有位置 |
| pager/src/app/agent_view/mouse.rs:417 | 仅 jump_to_turn 本地序号 | 无法请求未加载目标 |
| shell/src/agent/mvp_agent/history_paging.rs:33 | 请求仅 cursor，响应仅 updates/olderCursor | 缺少 around、after 和独立目录 |

后端已有可复用基础：HistoryPageCut 绑定 session/branch/cut/hash；PagePosition 为 record/part/offset；IndexedPages 支持有界 page_before。无需新建历史数据库。`grow/prompt_history` 会按文本去重，服务于输入建议，不能充当保留每次发生身份的时间线目录。

`grow/historyCursor` 的 record/part、historyMessageEvent/Item、messageId 和 toolCallId 能提供源身份；encoded response offset 与 UTF-8 text offset 单位不同，必须明确返回 fragment 范围，不能直接拼文本或把一个工具的多次更新当重复条目丢弃。

Minimal 的 native terminal 已打印行不可前插。归档 `2026-11-15-resume-from-working-state/paging-design.md:93` 已指出此边界；本次必须落实虚拟历史窗口与 native commit seam，不能重置 committed 标记冒充顺序插入。

## 4. lifetime、逻辑 Turn 与上下文占用未形成同义读模型

### 已有且应复用的部分

`chat-state/src/usage.rs` 已有 UsageLedger、模型/Agent 分项、完整/未知分类和 resume segments。`session-timeline` 主规范的 durable lifetime、幂等 attempt、Sideband 与 segment 要求应继续保留。当前静态核对未证明普通非 resume TUI 的 lifetime 汇总普遍漏算，不能以用户现象推断整套账本失效。

### 同一 Turn 跨挂起后结果漏算

- `chat-state/src/actor/state.rs:1451` 恢复 session_usage，但 `:1482` 无条件设置 last_turn_usage/prompt_usage=None。
- `shell/src/session/actor/turn/admission.rs:560` 在恢复点为 Model 时沿原 Turn/prompt 继续，未重建原 prompt 账本。
- `turn/mod.rs:600` 从当前 prompt_usage 投影结果；`turn/admission.rs:1581` freeze 后进入 PromptTurnOk、ACP result 与 TurnCompleted。

若同一 Turn 挂起前已结算 100、恢复后再结算 50，lifetime 可以正确为 150，而该 Turn 的终态结果只包含 50；恢复后无需再采样时还可能没有 usage。这是静态可达的确定缺口，仍需新增进程级回归证明。

主 attempt 的 AttemptUsageSettlement 已保存 captured_prompt_index；AuxiliaryAttemptStart 部分保存该归属。SubagentUsageSettlement 只有 subagent_id/by_model/incomplete，prompt 归属仅在实时 fold 的 attribute_to_prompt 决定，未进入持久载荷。完整恢复需要补齐这项归属，并在既有 working publication 保留可验证的活动 Turn 用量投影。

结果已有留存出口 `GrowHistoryFact::TurnCompleted { usage }`；TurnEvent::Ended 不含 usage，现有 usage 索引也没有按 Turn 查结果的键。无需新增结果日志，应修正原 Turn 用量并让原 completion 展示事实/索引按需返回正确结果。

### 上下文占用与消费口径混用

`shell/src/session/actor/updates.rs:208` 的 totalTokens 来自 get_projected_tokens；它是当前请求的校准上下文压力。`pager/src/acp/meta.rs:47` 却把该字段解释为 session accumulated。`chat-state/src/actor/state.rs:1495` 起的校准与 request projection 说明此值本应随压缩/裁剪/模型路由变化，不能用来代表累计消费，也不能按屏幕正文重新估算账单。

### 非 resume 的结果完整性丢失

`pager/src/headless/reducer/messages/usage.rs:35` 读取 usage_is_incomplete 后仅写 warning，ResultUsage/ResultLine 未传递该标记；`:74` 和 `:120` 把未知费用转换为 0.0。streaming-messages-json 结果因此把未知/部分账单呈现为外观精确的数值。普通 JSON 的直接投影路径不受这一处丢失影响。修复应局限于结果语义和序列化，不能据此更换底层结算公式。

## 验证边界

本轮新增方案和 delta，通过规范校验后交付。产品行为未修改，以上运行场景均待实施验证。实施验收须从实际 Ctrl+D/冷恢复、resident reconnect、滚轮/时间线和 ACP/headless 入口进入，分别比对持久结算、当前控制投影和可见结果；格式校验不代表这些行为已通过。
