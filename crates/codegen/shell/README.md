# shell

Grow 的会话、配置、模型路由与工具运行时核心。

用户文档、BYOK TOML 示例、Agent 规则和构建方式统一维护在仓库根目录的
[`README.md`](../../../../README.md)，避免子 crate 文档复制过时的上游安装、认证与网络端点。

本 crate 不内置模型。模型提供商、模型名、API 端点和密钥均来自用户配置；未配置可用
LLM 时，由前端引导用户打开配置。

## Session ownership

`SessionActor` 是会话的单一组合根，并作为 `!Send` actor 运行在 Tokio
`LocalSet` 上。它串行协调 ACP mailbox、Timeline 提交、foreground turn、工具调用、
通知和 Goal continuation；这些事实不得复制到第二个 actor 或前端状态中。

- `AdmissionState` 的单一 `TokioMutex` 同时保护 foreground owner、用户 FIFO、
  manual compaction admission、notification suppression 和 rewindable boundary。
  不得把这些字段拆成多个锁；任何调用都不得持有 admission guard 跨未知 `.await`。
- `McpSessionState` 只聚合会话级 MCP 策略、初始 server 配置、tool metadata、
  announcement/reminder 与 readiness 状态。foreground、Timeline notification 和 MCP
  进程生命周期仍由既有 owner 管理。
- `HookSessionState` 只聚合 hook registry、client hooks、workspace/VCS context 和加载
  错误。Plugin registry 保持独立，Hook 与 MCP 不合并为通用 extensions bag。
- `BehaviorCoordinator`、`GoalTracker`、`WorkflowManager`、`SessionMemory`、
  `EventTracker` 与 ChatState/Timeline 各自拥有其领域状态；`SessionActor` 只负责按既定
  顺序协调它们。

Idle admission 的优先级是用户 FIFO → durable notification → Goal continuation。
Timeline 是唯一持久事实源；foreground、projection 或 render cache 都不能成为第二份
持久事实。`RefCell` borrow、MCP/admission lock 以及 hook registry borrow 均不得跨未知
`.await`。

## Dependency direction

协议、权限、工具、MCP 和持久化能力可以被 session actor 调用，但不得反向依赖
`SessionActor` 的内部字段。工具调用的授权与 dispatch 继续围绕
`PreparedToolCall` / `ToolDispatchAuthority`，不会引入第二个 tool runtime 或 session
actor。
