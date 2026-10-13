## ADDED Requirements

### Requirement: MCP protocol upgrades use the official SDK integration
MCP 协议类型、编解码、discovery/协商和 MRTR 机制 SHALL 由官方 rmcp 提供，经 Grow 现有 mcp crate 单一入口集成。Grow SHALL 实现宿主调用归属、交互、权限、期限和持久化边界，不另写 MCP 协议栈、MRTR 协议循环或协议兼容框架。SDK 升级 SHALL 同步验证依赖/trait/feature 兼容及既有 TLS/platform 约束。

#### Scenario: SDK already provides MRTR and protocol negotiation
- **WHEN** 实现多轮交互并选择支持目标协议的 rmcp 版本
- **THEN** 使用 SDK 自动 MRTR/helper/handler 与协议协商能力，Grow 只补宿主适配；下游继续通过 mcp::rmcp 使用统一类型。

#### Scenario: SDK lacks a required host integration point
- **WHEN** 选定版本无法证明原 owner 关联、逐轮取消或其他必需宿主边界
- **THEN** 明确记录并通过上游扩展/修复/版本升级解决，在验证前不宣称支持，不退回自研协议循环。

实施入口：`crates/codegen/mcp/Cargo.toml`、`mcp/src/lib.rs`、SDK ClientHandler/transport 适配及已有依赖 workaround 记录。

### Requirement: MCP multi round-trip calls preserve logical ownership
MCP 集成 SHALL 在同一 logical tool call 内使用官方 SDK MRTR continuation 处理 InputRequiredResult；SDK 使用新的 wire request id 并按 input request key 回送回答，Grow 负责绑定宿主 owner 并提供交互回调。requestState SHALL 由 SDK 原样回送，仅用于原调用；本轮未提供 state 时不得沿用旧 state。模式与 transport 支持 SHALL 以实际能力声明为准。

#### Scenario: Two input rounds
- **WHEN** 原 tools/call 连续返回两轮 Form
- **THEN** 各轮使用不同 wire id，答案只归属原 call/key，最终只结算一份工具结果。

#### Scenario: Unsupported mode or empty continuation
- **WHEN** 服务端要求未声明支持的模式，或 input_required 既无非空 inputRequests，也无符合协议类型的 requestState 字段
- **THEN** 返回明确协议/能力失败，不挂起无法展示的询问或猜测答案。

#### Scenario: A later round omits state and reuses a key
- **WHEN** 上轮有 requestState，而本轮没有该字段且复用相同 input key
- **THEN** continuation 不携带旧 state 或旧答案，本轮回答按新的轮次/交互实例接纳；opaque state 的有效性只按协议类型判断，不解析其内容。

#### Scenario: Final result contains structured and image content
- **WHEN** MRTR 完成后返回 structuredContent、isError 或图片等 typed block
- **THEN** 沿既有结果投影、附件预算和单次 tool_result 结算，不仅提取 text 或暴露中间 input_required 给模型。

实施入口：`crates/codegen/mcp/src/servers.rs::McpTool/try_call_tool`；现有调用路径为证据，MRTR 为本 change 新增契约。

### Requirement: MCP elicitation distinguishes decisions from external completion
Form/URL 交互 SHALL 区分 accept、decline、cancel。Form accept SHALL 只携带明确提交且通过后端 schema 验证的内容。URL accept SHALL 仅表示同意外部交互，业务完成以服务端最终响应为准。

#### Scenario: URL remains pending after consent
- **WHEN** 用户同意打开 URL 后服务端仍返回 input_required
- **THEN** 原调用保持等待，提供检查或取消，不把打开页面或本地 Done 当成业务完成。

#### Scenario: Invalid form input
- **WHEN** 回答不满足必填、类型、枚举或大小限制
- **THEN** 后端拒绝该提交，不只依赖客户端验证。

#### Scenario: Optional fields and typed empty values
- **WHEN** 表单含可选未填字段、合法 false/0、单选或多选枚举
- **THEN** 未填与合法值分别处理，保留明确提交的类型和枚举值；不支持的 schema 明确拒绝，不静默降级、补字段或强制转换。

#### Scenario: Cancel one input without stopping the operation
- **WHEN** 同轮多个请求中，用户对一个 key 选择 decline/cancel 而未停止 logical call
- **THEN** 该 action 仅作为该 key 的回答，其他已接纳项仍可处理；停止调用是另一个明确动作。

实施入口：`crates/codegen/shell/src/session/pending_interaction.rs`、MCP typed response 与 Pager 交互适配；具体新类型待实现。

### Requirement: MCP elicitation cancellation is scoped to the original operation
交互及取消 SHALL 绑定 session、执行 owner、logical call、server 配置/transport 身份及交互轮次。取消 SHALL 只清理匹配调用的 waiter/continuation；活跃 wire 的取消使用原 peer/id。超时或结果未知 SHALL NOT 自动重放写调用，也不得宣称外部副作用已撤销。

#### Scenario: Replacement service reuses an RPC id
- **WHEN** 旧交互回答或取消在 transport 替换后迟到
- **THEN** 不应用到替代服务或新调用，旧错误也不清理新 transport。

#### Scenario: Network timeout while a write may have happened
- **WHEN** 写调用未取得最终响应而超过网络期限
- **THEN** 报告失败或结果未确认，不无条件重新派发同一写操作。

#### Scenario: Transport or server error after a possible send
- **WHEN** MRTR initial request 或 continuation 可能已经发送，随后连接断开、发送报错或返回服务端错误
- **THEN** 不进入既有 recover-and-retry 重放或回退 initial call；仅协议 input_required 授权正常续接，可证明未发送的重试除外。transport 恢复不等于原操作可重放。

#### Scenario: Admission closes while the user is answering
- **WHEN** 等待期间原 owner 被取消、工具禁用、transport 替换或 storage admission 确认关闭
- **THEN** 后续回答/检查不能派发新的业务请求；报告失效/受阻并保留必要终态事实，清理本调用资源，不影响其他 owner。

实施入口：`crates/codegen/mcp/src/servers.rs::call_tool_cancel_aware` 和已有 transport identity/recovery；新增交互必须沿用这些边界。

### Requirement: MCP user and external waits have distinct bounded lifetimes
MCP 客户端 SHALL 区分网络往返、人类输入和外部 URL 等待；人类填表时间不消耗单轮网络响应期限。外部轮询和未处理交互 SHALL 有界且可取消，不因重连、新轮次或手动检查重置 logical call 总期限。队列等待 SHALL 从接纳时计时；数量及 message/schema/URL/requestState/答案的单项和累计字节 SHALL 在接纳前受限。等待 SHALL 不持有阻塞回答、取消或其他会话进展的 actor/MCP 共享锁。

#### Scenario: Human reads longer than the network timeout
- **WHEN** 用户仍在处理一个有效 Form 且阅读时间超过单轮网络 timeout
- **THEN** 不误报服务端网络失败，仍受显式人类等待策略约束。

#### Scenario: State-only external wait
- **WHEN** 服务端连续返回只有 requestState 的等待响应
- **THEN** 客户端按有界退避或手动检查继续，并在总期限或用户取消时结束等待。

#### Scenario: Queued or oversized round
- **WHEN** 一轮超出字节/项数预算，或交互在队列中到达其等待期限
- **THEN** 明确结束该轮/调用并释放其预留，不部分挂起、不等待首次展示才启动期限；其他已接纳调用仍可回答和取消。

实施入口：官方 SDK MRTR helper/ClientHandler 的宿主适配、Shell pending lifecycle；具体容量与期限在实现前确定并验证。

### Requirement: MCP elicitation preserves data and authorization boundaries
Form SHALL NOT 收集密码、API key、访问令牌或支付凭证。URL SHALL 展示实际来源和可检查目的地，经显式同意后使用安全 opener 打开，不预取、不接受危险 scheme 或嵌入凭据。客户端 SHALL NOT 将中间交互的 requestState、敏感 URL 和表单正文自动复制进模型提示、普通 Timeline 正文、日志或遥测。服务端最终 tool result 仍沿既有结果规则处理，不据此承诺自动识别/过滤服务端回显的所有秘密。交互同意 SHALL NOT 放宽既有工具授权和 MCP eligibility。

#### Scenario: Secret requested in a form
- **WHEN** 服务端要求通过 Form 提交秘密凭据
- **THEN** 拒绝该收集路径，不把凭据转为普通回答或日志。

#### Scenario: Dead interaction attempts to open a URL
- **WHEN** URL 同意来自已失效的 waiter
- **THEN** 不打开地址、不使已结束调用恢复。

#### Scenario: Explicit data submission
- **WHEN** 用户提交通过校验的 Form
- **THEN** 仅向原已授权服务提交可见且确认的字段，不附加模型猜测或未展示的本地信息。

实施入口：MCP/Shell shared types、Pager URL/表单适配、现有 tool authorization gate；不得以 UI 文本替代连接身份。

### Requirement: MCP interaction capabilities are scoped to the calling entry point
现代协议 initial/continuation 请求 SHALL 携带必需的 `_meta` 版本、客户端信息和实际 clientCapabilities。Form/URL 声明 SHALL 以原调用可用的交互路由为依据，不借用其他可见会话。未支持的 input method SHALL 在展示整轮交互前明确拒绝，不隐式启动 sampling、读取 roots 或拓宽权限。

#### Scenario: Another session has an interactive client
- **WHEN** direct、无 session、headless 或子会话调用没有自己的有效交互路由，而另一会话有 Pager
- **THEN** 原调用不因此声明 Form/URL，也不把询问交给那一会话；违规 input_required 明确失败而不挂起。

#### Scenario: Mixed unsupported input methods
- **WHEN** 同一 inputRequests 同时包含 Form/URL 和未声明支持的 sampling/roots/未知 method
- **THEN** 整轮在打开 URL 或收集答案前失败，不部分执行嵌套请求；既有独立能力不由本 change 自动开启。

实施入口：MCP request metadata、Shell 工具上下文及 `shell/src/extensions/mcp.rs` direct 入口；复用同一调用实现。

### Requirement: MCP answers and continuation dispatch have one runtime owner
同一交互实例的回答 SHALL 单次接纳，同身份同内容重试返回原状态，冲突或旧轮次回答拒绝。每个 logical call SHALL 至多一个 continuation 在途；手动检查、自动检查和重复客户端操作共用该边界。

#### Scenario: Duplicate answer and competing checks
- **WHEN** 两个客户端重复提交同一回答，同时手动检查与自动检查竞争
- **THEN** 仅一次回答接纳与一次 continuation 派发，重复不新增外部调用；不同回答不覆盖首个已接纳结果。

实施入口：原 MCP continuation owner 与 Shell typed response，不新增 durable 答案存储或调度 owner。
