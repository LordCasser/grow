## Why

Sideband 咨询目前把回答交给提问方，但回答方前台只有 UI/审计记录，没有对应模型输入。回答方后续可能把真实问答误判为虚构。用户确认采用类似 DSH `inject` 的投递语义，并要求从 `/Users/lordcasser/workspace/projects/deepseek-harness` 核对执行控制与上下文边界的分层。

Grow 已有 durable Input、Notification、exact TurnId 和 Step 控制栅栏。本提案复用这些所有者，首个实现增量限定为成功咨询问答的被动同步；不引入新 Agent loop、消息数据库或四套同名产品 API。

当前状态：demo、询问回写实现、联合旅程验证和独立对抗性审查已完成；最终静态检查、归档及本地 release 的实际结果见 [收口记录](release-verification.md)。早期运行过程保留在 implementation-verification.md，实际旅程分析见 user-journey-audit.md。

## What Changes

- 将现有消息相关入口按流程控制动作与上下文边界操作分层，共用已有接纳和边界协议。其他上下文变更不归消息投递管理。入口映射与对抗反例见 [消息投递分层](mental-model.md)。
- 成功的 `ask_parent`、`ask_subagent`、`ask_session` 在回答方形成有归属的、持久化的被动上下文来源，按安全边界恰好消费一次。
- 被动同步不取消模型请求、不打断工具、不主动唤醒空闲会话；空闲、取消收敛和挂起期间保留待消费事实。
- 调用方继续使用已有工具调用/结果；回答方输入注明 Sideband 来源、问题和完整回答，不冒充前台 Assistant 发言、人类指令或权限批准。
- 保留 Grow 用户 steer 的当前软抢占产品行为，但明确它是“当前请求抢占 + 同 Turn 投递”的组合，不能复用于被动同步。
- 使用既有成功终态、Notification Received/Consumed、不可变 payload 和单 writer 恢复路径；补齐终态与收据之间的崩溃恢复。

## Capabilities

### New Capabilities

无。内部按职责分层，不新增通用控制平台。

### Modified Capabilities

- `local-coordination`：咨询执行仍隔离，成功结果事后同步回答方；调整现有禁止自动写回的要求。
- `input-admission`：投递、唤醒与抢占分开决策，保留用户输入身份、权限和精确目标约束。
- `session-timeline`：被动上下文的唯一来源、消费原子性和恢复边界。

## Impact

涉及 Shell 的 inquiry 完成/去重/恢复、通知投递与安全消费，ChatState 的通知来源与 Timeline 验证，以及模型请求中的归属/权限投影。首版不修改快捷键、用户 FIFO 优先级、Stop/Close/Delete、Goal/Plan/Workflow 所有权或现有显式 steer 失败语义。实现时同步 `docs/architecture/input-routing.md`、`docs/architecture/local-coordination.md` 和受影响开发者说明。

依据见 [源码对照](evidence.md)、[设计](design.md)、[任务与验收](tasks.md)。Tier 3 联合证据和最终独立 review 见上述收口记录；各阶段结果按原发生顺序保留。
