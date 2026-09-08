## ADDED Requirements

### Requirement: Idle control completion refreshes Behavior availability
Shell SHALL 在空闲会话的 model、Agent 或 Behavior 控制结束并释放前台占用后发布当前 Behavior 可用性，客户端不能永久保留临时忙碌投影。

#### Scenario: 尚未发送消息时切换控制
- **WHEN** 用户在尚未发送消息的会话完成 model 或 Agent 切换
- **THEN** 支持的 Goal 和 Workflow 选择按当前空闲事实可用，无需先发送或停止 turn。

#### Scenario: Behavior 控制完成
- **WHEN** 空闲会话完成 Behavior 切换
- **THEN** 后续选择依据释放控制占用后的可用性。

证据范围：`crates/codegen/shell/src/session/actor/model_switch.rs`、`session_mode.rs`。
