## ADDED Requirements

### Requirement: Agent communication tools expose their intent and result

通信工具的发送侧 SHALL 从开始到终态显示具体工具身份、实际目标、消息或问题预览及结果语义。展开 SHALL 保留完整正文、相关身份、投递模式与错误。工具行 SHALL 使用其原始调用身份原位更新，不能另加重复发送通知。

#### Scenario: Parent sends queued guidance
- **WHEN** 父 agent 调用非中断 `send_subagent_message`，随后收到 durable receipt
- **THEN** 同一行从发送中更新为已接收，并展示工具名、目标子任务、原文预览及下个安全步骤加入上下文的模式，不显示已读或任务完成。

#### Scenario: Immediate guidance and uncertain acknowledgement
- **WHEN** 父消息请求安全中断，或者等待回执超时而消息可能已持久接收
- **THEN** 展示分别说明请求安全中断或投递状态未知，不推断非中断工具已停止，不自动重发消息。

#### Scenario: Parent child or peer inquiry
- **WHEN** `ask_parent`、`ask_subagent` 或 `ask_session` 开始并结束
- **THEN** 同一发送工具行展示实际对端、问题预览和回答或失败，细阶段缺失时只显示等待回答，展开保留原问题与完整结果。

#### Scenario: Inquiry state lookup
- **WHEN** `get_inquiry` 查询成功且返回一个失败的 inquiry
- **THEN** UI 区分查询完成和 inquiry 失败，不把已查询到的失败结果冒充查询工具调用失败。


### Requirement: Parent message receipt appears in its child view

子 agent SHALL 在父消息持久接收后显示一条具有稳定事件身份的系统接收通知，包含父 agent 来源、投递模式与原始消息。通知 SHALL 属于实际接收 session，且不得作为人类输入、子 agent 自己的工具调用或额外模型上下文。接收通知 SHALL 不依赖消息被消费或当前视图是否选中。

#### Scenario: Receipt while the child is busy
- **WHEN** child 持久接收父消息而仍在执行当前步骤
- **THEN** child TUI 可见一条接收通知和正文预览；当前工作继续遵循原投递模式，UI 不抢焦点。

#### Scenario: Duplicate or replayed receipt
- **WHEN** 同一收件事实重试、重连回放或已消费后冷恢复
- **THEN** 接收视图保留恰好一条可读收件通知，不重新消费消息或重新触发通知副作用。

#### Scenario: Receipt projection fails after commit
- **WHEN** inbox 已持久接收而 UI 投影未成功发布
- **THEN** 投递回执仍反映真实接收，重载可从持久事实重建通知，不因此重复发送消息。

#### Scenario: Normal and minimal history
- **WHEN** 父消息送往未选中的子视图，随后在 normal 或 minimal 模式查看或恢复
- **THEN** 接收记录在所属子视图可见，minimal 原生历史追加该不可变收件事实一次，主 turn 结束不影响独立问答接收行的生命周期。


### Requirement: Communication presentation preserves readable text

通信记录 SHALL 默认提供有界正文预览，保留可展开、选择和复制的完整文本。工具、参与方、状态与消息正文 SHALL 能在窄终端中辨认，状态不得仅通过颜色区分。

#### Scenario: Long Unicode message containing an image path
- **WHEN** 消息含中文、多行内容、长任务名或本地图片路径
- **THEN** 预览按终端显示宽度换行和明确截断，展开保留原文，图片引用不会替代整条消息文本。

#### Scenario: Keyboard access to communication details
- **WHEN** 用户只使用键盘选择通信记录并进入现有详情入口
- **THEN** 可以阅读和复制完整消息、方向及结果，无需鼠标操作。

