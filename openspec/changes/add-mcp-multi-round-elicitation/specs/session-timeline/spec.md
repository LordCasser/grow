## ADDED Requirements

### Requirement: MCP human responses remain attached to the original tool call
MCP 交互响应 SHALL 绑定原 session/工具调用/执行 owner 和交互身份，不转换为普通 Prompt、Steer 或 Followup，不额外运行 UserPromptSubmit Hook。需要持久化的关联与结果 SHALL 由现有 Timeline owner 记录最少事实，实时 pending registry 不成为第二套 durable 调度权威。

#### Scenario: A second elicitation round is answered
- **WHEN** 同一工具调用第二轮收到用户回答
- **THEN** 继续原调用，不追加新的 HumanIntent 或第二个模型工具调用；最终 tool_result 只发布一次。

实施入口：`crates/codegen/chat-state/src/timeline.rs`、`shell/src/session/actor/input_admission.rs`、现有工具终态记录；新增 schema 形状须在实现时显式评审。

### Requirement: MCP reconnect does not fabricate durable continuation
界面重连 SHALL 仅恢复可验证仍由当前运行 owner 持有的交互。首版冷启动恢复 SHALL 将无法验证活跃性的未完成交互标明中断或结果未确认，不依据遗留 URL、答案或 requestState 自动重放工具。

#### Scenario: UI reconnects while the shell survives
- **WHEN** 原 Shell owner 和交互 waiter 仍可验证有效
- **THEN** UI 绑定同一交互，不重新派发工具。

#### Scenario: Shell exits during external interaction
- **WHEN** 原运行 owner 已退出且没有经验证的跨进程 continuation 契约
- **THEN** 恢复只展示已知事实与限制，不伪造完成或自动重试写操作。

实施入口：Shell pending/restore 与 Timeline tool 事实；跨进程自动续接不在本 change 范围。
