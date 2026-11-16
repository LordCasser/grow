## Why

Ctrl+D/resume 后，运行时的当前控制状态、用户可浏览的历史和 token 展示没有形成完整的恢复闭环；Plan 还在普通权限之前额外禁止终端调查。方案与根因调查已交付，用户现已授权在一轮方案盲区复查后实施。

## What Changes

- 从已恢复的会话控制 owner 发布当前 Behavior 和 Plan phase；冷恢复、resident reconnect 与最新历史页分别完成，历史分页不能覆盖当前状态。
- **BREAKING**：Plan 各阶段使用与 Normal 相同的普通工具权限链，移除基于 Plan phase 的 Bash/Edit/MCP/子 Agent 能力禁用。强化 Plan 提示词：规划阶段的一切工具调用只服务于形成、验证和修订计划，禁止直接实施或通过脚本、MCP、子 Agent、Workflow 绕过审批；工具权限放行不等于计划获批。Goal/Workflow 的互斥与归属沿现有协议。
- 将最近历史页、向前/向后分页、时间线目录和按位置加载窗口接到同一会话历史截点。滚动加载保留位置，点击未加载的时间线条目直接加载目标附近历史。
- **BREAKING**：当前上下文 metadata 从 `totalTokens` 更名为 `contextUsedTokens`，消费结果的 `totalTokens` 含义不变。稳定 `TurnId/prompt_id` 归属及活动 UsageCheckpoint 保留同一逻辑 Turn 的主、辅助与子 Agent 结算；完成用量随原 TurnCompleted 冻结留存，浏览历史不参与消费累计。
- 保留已归档的有界分页、稳定身份、执行恢复 gate、缓存分类未知和幂等结算契约；实现中修正与这些契约不符的路径。

## Capabilities

### New Capabilities

无。复用现有 session、Timeline、权限和展示模块。

### Modified Capabilities

- `behavior-goal`：当前 Behavior 的恢复投影，以及 Plan 规划阶段工具用途和禁止绕过审批的协议。
- `tool-authorization`：Plan 与 Normal 共用普通工具权限，消除基于 Plan phase 的额外执行禁用。
- `client-surfaces`：段式历史浏览、独立时间线目录、位置跳转和当前状态/统计不随分页回退。
- `session-timeline`：用量读模型与执行恢复、分页展示之间的一致性及结算留存。

## Impact

涉及 `shell` 的 SessionActor、load/history ACP 扩展，`chat-state` 的已有历史索引与 Usage 投影，以及 `pager` 的 AgentView/scrollback/timeline、当前控制和用量 reducer。候选接口在 `design.md` 中约定；不增加运行时依赖，不新建第二份会话状态或 token 账本。

实施同步 `docs/development.md`、Pager Plan/权限与用量说明；完成场景验收后归档，不在验收前将候选行为写入主规范。Git 提交仍不在本次授权中。
