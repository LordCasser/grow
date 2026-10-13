## ADDED Requirements

### Requirement: MCP interaction queues preserve every admitted owner
客户端 SHALL 在现有 pending 管理下有界排队 MCP 交互，展示服务来源、调用目的及待处理数量；新请求不得静默替换正在填写或已接纳的请求。取消只移除匹配 logical call 的项，溢出返回明确结果。

#### Scenario: Parallel elicitation requests
- **WHEN** 多个工具并行请求输入且队列仍有容量
- **THEN** 按稳定接纳顺序展示，各 responder 只结算一次，回答不串到别的调用。

#### Scenario: Queue capacity is exhausted
- **WHEN** 新交互超出配置或实现规定的有界容量
- **THEN** 明确拒绝/取消新交互并说明原因，不静默取消旧交互。

#### Scenario: Several inputs share one tool call
- **WHEN** 一个 logical call 同轮有两个 key，或旧轮次 guard 在新轮次接纳后释放
- **THEN** 每项有独立交互实例，旧 guard 只移除其仍拥有的项，不覆盖同 call 的另一个 key 或新轮次 responder。

实施入口：`crates/codegen/shell/src/session/pending_interaction.rs` 和 Pager 会话交互接纳。

### Requirement: MCP cards preserve focus and user drafts
MCP 交互 SHALL 复用现有权限优先级、会话路由及集中键盘 owner；后台会话不得夺取当前焦点。草稿 SHALL 由拥有该次交互的 UI 保存和恢复，焦点移动与取消询问语义分离。

#### Scenario: Permission and MCP overlap
- **WHEN** 权限提示、计划审批和 MCP 卡片同时存在
- **THEN** 既有优先级继续生效，原输入草稿不被空值覆盖，未显示交互仍保有其 responder。

#### Scenario: Background session needs input
- **WHEN** 后台 session 收到 MCP 询问
- **THEN** 对应会话显示 NeedsInput，不把请求路由到当前其他会话。

实施入口：Pager `app/agent_view` 和 ACP interaction dispatch；保留 Grow 最顶部 task 布局，不引入统一 Dock。

### Requirement: MCP form and URL surfaces expose the actual decision
客户端 SHALL 提供 Form 验证与提交/拒绝/取消，以及 URL 同意、等待、检查、重新打开和取消；URL waiting 的收起仅改变展示，不伪造服务端完成。非交互客户端不得声明无法处理的 elicitation 模式。

#### Scenario: Collapse an external wait
- **WHEN** 用户收起 URL waiting 卡片但未取消原调用
- **THEN** 调用继续保持真实等待状态，仍可找回控制入口，不生成成功结果。

#### Scenario: A form arrives behind an external wait
- **WHEN** URL 已同意并等待外部完成，另一个调用的 Form 已接纳
- **THEN** waiting 释放键盘焦点，Form 可按队列规则处理；原 URL 保留检查/取消入口，不因展示切换重新打开 URL。

#### Scenario: Interactive bridge is unavailable
- **WHEN** 当前客户端不能交付所需用户交互
- **THEN** 返回明确不支持或取消，不无限等待不可见询问。

实施入口：Pager 表单/URL 视图与 Shell capability/bridge。
