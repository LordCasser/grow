# v2.1.0 临时待办

> **Status**: In progress
>
> **Start gate**: 等待并行工作完成；只有用户明确指令才能启动，不自动轮询或推进。
>
> **Lifecycle**: 全部任务验收后，把稳定契约迁回对应架构文档并删除本文件，避免它成为第二份长期事实源。
>
> **Source**: `docs/architecture/upstream-borrow-review.md` 的第二轮评审。

本文件只放当前版本可以做、但还没有获准启动的任务。MCP Elicitation、Worktree 生命周期和底部 status line 均不在这里。

## Behavior 与 Workflow 架构债务

以下问题来自控制流审计，但不属于本次 Plan/Goal/Workflow successor 重构，需单独设计和验收：

- [x] **Workflow Inspect/Search 访问投影**：当前接口声明为 Read，但 Inspect 会持久化 focus，Search 打开 workspace 时也可能恢复 publish、更新 hash 或清理 draft。后续应让访问分类覆盖真实持久化副作用，并用 action 矩阵测试锁定权限。
- [x] **Plan 问答底栏闭环**：`ask_user_question` 声明的 “Chat about this / Skip interview” 在 Pager 没有完整的键盘、渲染和 typed-response 路径。后续应删除无效入口或补齐端到端交互，不再保留半连接状态。
- [x] **Workflow Timeline-only 恢复**：恢复目前以 manifest 为入口；manifest 缺失或损坏时，即使 Timeline 已有 Spawned/Ended 事实也无法重建 run 投影。后续应明确 Timeline 权威性并覆盖 manifest 写失败后崩溃的恢复测试。
- [x] **Behavior availability 与 busy/确认语义**：选择器展示的可用状态可能在实际请求时先被 foreground busy guard 拒绝。后续应让展示、确认窗口和服务端 admission 使用同一份判定结果。
- [x] **Pager 本地草稿恢复**：未发送 prompt 和 deferred mode 只存在客户端内存，崩溃后丢失。后续应设计独立的本地草稿持久化边界，不能混入 Shell 的权威会话状态。
- [x] **Control 快照锁作用域**：部分既有持久化入口会让 Behavior/Goal 的同步锁 guard 跨越异步 Timeline 写入。后续应统一先复制权威快照再释放锁，并用并发持久化失败测试证明不会阻塞状态推进。

## Hook 全生命周期进入 Timeline

这项任务不是单独给 `UserPromptSubmit` 加一个阻断回调，而是先让 Hook 成为 Timeline 中的闭合事件族。`HookEventName::ALL` 当前覆盖：

`SessionStart`、`UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`PostToolUseFailure`、`PermissionDenied`、`Stop`、`StopFailure`、`StopCancelled`、`Notification`、`SubagentStart`、`SubagentStop`、`PreCompact`、`PostCompact`、`SessionEnd`。

目标生命周期：

```text
HookTriggered
  → HookRunStarted
  → HookRunFinished / HookRunSkipped
  → HookCompleted
```

- [ ] 增加 typed Hook Timeline family，并一次性提升 Timeline schema；不维护旧 schema 的兼容读取路径。
- [ ] 每次真实 Hook occurrence 都先写入 `HookTriggered`。没有 handler、全部 matcher miss、被策略禁用也必须以明确终态闭合，不能在 `hook_event_active` fast path 静默消失。
- [ ] `HookTriggered` 记录事件类型、因果来源引用、配置 generation/provenance，以及按确定顺序考虑的 handler 和匹配结果。未来增加 `HookEventName` 时，穷举映射和表驱动测试必须失败，不能让新事件绕过 Timeline。
- [ ] 每个实际启动的 handler 使用唯一 `run_id`，并且恰好一个终态。结果区分 `success`、`blocked`、`failed`、`timed_out`、`cancelled` 与 `skipped(reason)`。
- [ ] `HookCompleted` 保存聚合决定。Observe、Prompt、Tool、Stop 使用各自 typed decision，不用同一个含义不清的布尔值。首次决定性 block 后停止执行后续 handler，并记录 `skipped(prior_block)`。
- [ ] Trigger 在外部进程或 HTTP 请求前持久化；run 结果和聚合决定在模型、工具、路由或 stop 行为继续前持久化。崩溃恢复把未闭合 run 记为 interrupted/outcome-unknown，不自动重跑可能已经产生副作用的 Hook。
- [ ] Hook 在 child session 发生时只写 child Timeline，父 Timeline 不复制其内部 Hook 细节。
- [ ] Timeline 保存规范化结果和真正影响控制流的内容。已有 tool/message/source 事实用引用关联，不复制完整 `toolInput`/`toolResult`；展开后的 URL、环境变量和其他潜在秘密不能落盘。
- [ ] TUI Hook annotation、Trajectory 与 diagnostics 改为 Timeline 投影。现有 `HookExecution` transport 可以继续承载实时投影，但不能保留为并行事实源。

### UserPromptSubmit admission / block

输入先成为事实，Hook 再决定它能否进入原有路由：

```text
InputSubmitted（durable，尚未路由、尚未进入 Surface）
  → 完整 Hook Timeline lifecycle
  → InputAdmissionResolved
  → Allow：同一 input_id 进入 Grow 原有 idle/queue/steer 路由
  → Block：保留输入和决定，不创建 turn、queue 或 interjection
```

- [ ] 所有真实 `HumanIntent` 都执行 admission，包括普通输入、忙碌期间 follow-up 和显式 steer。同一 `input_id` 只运行一次 Hook，排队、提升和重新仲裁不能重复触发。
- [ ] 被阻断输入可在 transcript/Trajectory 中审计，但永远不进入模型 Surface。Goal、Workflow、Notification 等合成来源仍记录 Hook 触发和结果，但强制 observe-only。
- [ ] `UserPromptSubmit` 与 `PreToolUse` 支持每 handler 的 `on_failure = allow | block`，默认 `allow`。Observe、Stop、SubagentStop 上出现该字段时直接报配置错误。
- [ ] Hook block 只拒绝对应输入，不冻结既有 FIFO 或 active turn。admission 没有得到 durable Allow 时，不允许产生模型调用、工具执行或路由副作用。

### Hook 验收

- [ ] 对当前全部 15 种 Hook 类型做表驱动 Timeline 覆盖测试。
- [ ] 覆盖无 handler、matcher miss、成功、阻断、失败、超时、前序阻断跳过与恢复中断。
- [ ] 锁定 `input → trigger → runs → result → admission decision → 原有路由` 的事件顺序。
- [ ] 证明 blocked input 不进入 Surface，允许输入仍只走 Grow 的既有路由。
- [ ] 证明 TUI、Trajectory 和 diagnostics 能仅凭 Timeline 重建 Hook 展示。

## 采样、截断和恢复对照

先在 `docs/architecture/truncation-recovery.md` 建立恢复行为对照表，再根据差异决定是否产生独立实现项。已有语义只补回归测试，不借审计重写恢复架构。

| 场景 | Grow 应保持的行为 |
|---|---|
| `Length + text` | 保存清洗后的部分文本，沿同一 turn lineage continuation |
| 未完成 reasoning/tool call | 不进入后续请求，不执行 |
| 完整 tool call + `Length` | 完整调用优先，恰好执行一次 |
| context overflow | compact 后仍超限则 typed terminal，不进入 length continuation 循环 |
| `pause_turn` | 保存完整 assistant 输出，无合成 prompt 地 resample |
| provider stop reason | 同时保留原始 reason 与 Grow provider-neutral typed reason |
| transient failure | 只在没有不可逆输出时沿同一 lineage 重试 |
| Empty/cancel/late completion/replay | 保持独立终态；恢复不得复制工具调用或输出 |
| model family switch | 不自动 compact，除非已有可验证的 carrier 不兼容 |

- [ ] 逐行核对当前实现与测试，已有行为标记为 covered。
- [ ] 只为实际缺口拆分任务；不得把上游 provider-specific 映射直接变成 Grow 的内部语义。

## 子 Agent follow-up 契约审计

不移植上游 follow-up 工具或通道，只审计 Grow 当前 child admission：

- [ ] ownership 与 liveness 分开判断。
- [ ] Accepted 只在消息原子进入 child admission/Timeline 后返回。
- [ ] 使用稳定 message id，并区分 accepted、rejected、unconfirmed。
- [ ] saturation、deadline、payload limit、channel closed 使用 typed outcome。
- [ ] 不增加全局通道，不复用父 Agent FIFO。

如果现状不满足，只记录独立设计债务，不在本任务内实现新的 follow-up 功能。

## 安全威胁模型与回归项

- [ ] 验证低权限阶段写入的 Hook 配置不能在高权限阶段未经重新校验后执行。
- [ ] 验证仓库信任不会传递给后来 clone、替换或新建的目录。
- [ ] 验证凭据绑定服务身份与目标 authority。
- [ ] 验证 MCP 状态绑定 server identity、transport 与 config generation，旧 episode 不污染新实例。
- [ ] 验证低权配置不能关闭高权策略；只使用 Grow 自身 provenance 层级，不新增上游 managed-hook 体系。
- [ ] 验证沙箱、配额与网络拒绝保留 typed terminal 语义，不被误判为可重试采样错误。
- [ ] 将 socket mask、异步 I/O 等绕过面加入威胁测试输入，但不移植上游沙箱架构。

## 完成条件

- [ ] 所有已启动任务都有对应架构契约和回归测试；审计发现的额外债务已经拆出，没有混入原任务。
- [ ] 稳定结论已经迁回各自权威架构文档。
- [ ] 删除本文件，不保留第二份完成态任务账本。
