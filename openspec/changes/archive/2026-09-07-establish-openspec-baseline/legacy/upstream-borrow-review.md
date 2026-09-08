# Upstream Borrow Review（grok-build fork 后更新对照评审）

> **Status**: Review conclusions（非实现记录）
> **Date**: 2026-08-14
> **Scope**: 2026-07-28 ~ 2026-08-13 的 upstream/xai-org/grok-build 16 个同步提交
> **Author**: software-architect

本文记录 grok-build 分叉后更新中"可借鉴项"的逐项架构评审结论。它是后续实现任务（coder handoff）与 review 的依据；已采纳项的最终实现事实以各模块契约文档为准，本文只保留决策与排除理由。

## 0. 过滤原则

1. **本地优先**：功能价值若来自 xAI 云端平台（gateway、托管会话、订阅计费、发布式 bundle），不引入。
2. **单一权威来源**：grow 已有同语义机制时，只做审计与导出，不新建平行抽象。
3. **单一调度语义**：任何改动不得引入第二条 turn admission/interjection 路径（`v1.0.0-regression-analysis.md` 的教训）。

## 1. 已排除项（无实现任务）

| 上游功能 | 排除理由（已核实事实） |
|---|---|
| 图像预算滞回（47MB 触发/25MB 回收） | `chat-state/src/actor/request_builder.rs` 已有 `IMAGE_COMPACT_TRIGGER_BYTES`、低水位回收、per-turn 记录 |
| 401 认证归因（构建时捕获凭据） | `sampler/src/attribution.rs` 已有 `Auth401AttributionCallback`（callback 注入，比上游跨 crate 依赖更解耦） |
| 413 图片剥离重试（扣预算不受阻） | `sampler/src/retry.rs` 已有同语义 |
| GROK_EXTRA_CA_BUNDLE | grow 走 OS 信任库（rustls-native-certs/platform-verifier）；私有 CA 场景由系统信任库/`SSL_CERT_FILE` 覆盖；上游该能力服务于其托管 gateway |
| UsageLimit（云端 billing）tab | grow 无 billing/credit_balance 概念（零引用），/usage 为本地 token 统计 |
| telemetry OTLP / mixpanel | xAI 商业遥测，无需求来源 |
| foreign-sessions 发现 | 读取第三方产品状态，无需求来源 |
| workspace-daemon / diag-server / preview-proxy | grow 无 workspace-server daemon 架构 |
| bundle 缓存 | 上游 grok.com 发布式 bundle；grow 的 plugin-marketplace 是不同模型 |
| conv/<id> 分支绑定 | grow 已有 worktree + workflow-workspace 所有权体系 |

## 2. T8 评审：FitRung 五级阶梯 vs 本地 pre-prune 阶梯

**结论：不补"丢最老 history turns"级。** 本地结构已覆盖上游五级的全部能力，且语义保留严格更优。

对照：

| 上游 FitRung 级 | grow 对应物 | 结论 |
|---|---|---|
| verbatim | 不裁剪直传（现有路径） | 等价 |
| 丢最老 history turns | 无直接对应——grow 用 **summary 路径**（LLM 总结保留语义） | 不补。上游敢丢是因为其历史 server 托管可重拉；grow 历史是本地唯一副本，丢弃 = 永久信息损失。总结路径语义保留严格优于丢弃 |
| 前缀裁剪超大 tool result（max_bytes = tokens×4） | `common/compaction/prune.rs` pre-prune 阶梯（model-free，`plan_tool_result_pruning`）+ `item.rs` `truncate_payload_for_compaction` | 已有 |
| 丢最老 step turns | summary 路径的输入裁剪 | 已有 |
| emergency 硬缩最新项 | `item.rs` emergency tail shrink + `truncation-recovery.md`（D1-D8） | 已有 |

上游五级是**纯 token 拟合阶梯**（server 代理硬约束驱动）；grow 是"model-free 裁剪 → 语义总结 → 紧急兜底"三层，约束来源不同（本地持久化 vs 代理 413），结构不能直接对照移植。若未来出现"总结路径本身超限"的可观察案例，再沿 `run_compact_inner` 的 emergency 分支评估。

## 3. T9 评审：work_policy / response_guidelines 模板重组

**结论：不重组模板。** 本地 `prompts/foundation/mandatory-core.md` 的分层已覆盖上游重组的语义：

- 上游 `<work_policy>` 的核心语义（按可逆性/影响面权衡、授权边界、保护用户工作）已在本地 `<action_safety>` 完整表达，且更精确（"One approval is not blanket approval"、"Preserve work that may belong to the user"）。
- 上游 `<response_guidelines>` 的"禁止自创缩写/术语"是弱形式的增量：本地 `<output>` 已有 "prefer accessible language over filler, repetition, or unnecessary jargon"，缺"只用对话中已建立的词汇"这一强约束。

可选增量（非本清单任务）：如未来观察到代理自创术语，在 `<output>` 补一行 "do not coin abbreviations or terminology; use only vocabulary already established in the conversation"。

## 4. T1 重定位：readOnly 标注 → ToolKind 审计

**当前裁决**：旧的 `workspace::CapabilityMode + ToolKind` 过滤已经删除。`ToolKind` 只服务模板、UI 分组与发现；授权的唯一事实来自工具 descriptor 的 `max_access`、冻结参数的调用级 RWX 投影、actor 的 exact-identity eligibility/grant，以及一次性 permit。MCP 由 server trust-domain mask 与 transport generation 绑定。继续用 `kind` 推导权限会重新制造第二权威来源。

**T1 实际落点**：契约测试同时钉住所有内置工具的 kind（展示语义）和 descriptor RWX（授权天花板）；MCP/custom 的 kind-less config id 作为显式例外保留，但不会因此获得或失去执行权。主 Agent、普通 subagent 与 Workflow subagent 最终都在同一 dispatch Gate 消费调用级权限事实。

## 5. T2 重定位：StopCancelledReason → 复用 CancellationCategory

**已核实**：grow 的 `shell/src/session/event_types.rs` 已有 `CancellationCategory { HookDenied, PermissionRejected, PermissionCancelled, PermissionTimedOut, MidTurnAbort }`——turn 取消分类的权威来源已存在。上游 `StopCancelledReason` 中的 MaxTurns/NoProgress 是 gateway 概念，grow 无对应。

**T2 实际任务**：hooks 宏表加 `StopCancelled` 事件（Observe-only），payload reason 字段直接序列化现有 `CancellationCategory`（取消路径若缺失用户显式取消的分类，补 `UserInterrupt` 变体）；emit 点在取消分类已知处（`tasks_cancel.rs` 附近）。

---

## 6. 第二轮评审：`eb267fef..bc7f02ed`

> **Status**: Review conclusions（非实现记录）
>
> **Date**: 2026-08-31
>
> **Scope**: 2026-08-15 ~ 2026-08-28 的 11 个 upstream/xai-org/grok-build 同步提交
>
> **Task split**: 长期项进入根目录 `ROADMAP.md`；当前可做项进入 `TODO-v2.1.0.md`，等待用户手动启动。

这一轮的变化很多，但真正需要判断的不是“上游又多了哪些功能”，而是这些变化解决的机制问题在 Grow 中是否也存在。Grow 已经在 Timeline、foreground routing、权限 provenance 和 child session 所有权上完成分叉，所以结论仍然是吸收约束和失败经验，不同步另一套控制面。

### 6.1 评审边界

第二轮继续使用三个过滤条件：

1. **Timeline 是唯一事实源**：影响恢复、控制流或用户可见历史的事件必须进入 Timeline；TUI、diagnostics 和 transport update 只能做投影。
2. **输入只走一个路由**：Hook 可以决定一个输入是否获准进入路由，但不能自己建立 queue、turn 或 interjection 通道。
3. **权限模型不换轨**：上游安全修复可以转成 Grow 的威胁模型和回归测试，不能借此引入 managed config、平台 trust 或新的沙箱 owner。

本轮结论：

| 上游方向 | Grow 判断 | 落点 |
|---|---|---|
| MCP Elicitation、HITL UI | 协议目标仍在变化，现在绑定会把 draft wire 固化进核心 | 长期 ROADMAP |
| `UserPromptSubmit` block | 问题真实存在，但上游的提交顺序和 queue hold 不适合 Grow | v2.1.0 临时待办，按 Timeline 重做 |
| Hook 执行可视化 | 当前 `HookExecution`/diagnostics 不是权威事实 | 并入 typed Hook Timeline family |
| 截断、恢复和 transient retry | 大部分机制 Grow 已有，重点是终态与恢复边界 | 行为对照与回归审计 |
| active subagent follow-up | 架构已经分叉，不移植工具和通道 | 只审计投递契约 |
| `[ui.status_line]` | 是输入区域底部附加行，不是顶部 Agent 状态栏 | 搁置，不进入 TODO |
| Worktree 生命周期与复用 | 上游仍在快速改动，且不是当前关键路径 | 长期观察 |
| trust、credential、sandbox 修复 | 风险类型可复用，权限实现不可直接同步 | 威胁模型与回归测试 |

### 6.2 MCP Elicitation 解决什么问题

Elicitation 处理的是“MCP server 在一次调用尚未结束时，需要用户补充结构化输入或完成外部交互”的场景。Form 模式适合参数确认、选项和补充信息；URL 模式适合必须离开客户端完成的授权或确认。它避免 server 把问题伪装成 tool result 再让模型猜，也避免每个客户端发明一套私有弹窗协议。

上游实现把 MCP `elicitation/create` 转成 HITL pending interaction，再由客户端弹窗返回 accept/decline/cancel；URL 模式还要处理 server abandonment 和外部流程完成。但是当前实现有三个不能带入 Grow 的部分：

- `ElicitationInbox` 是 single-slot，新请求会取消旧请求，无法表达 Grow 已有 pending interaction 的多请求所有权。
- shell 与 UI 之间使用 xAI 私有 ACP 消息，不能成为 Grow 的协议边界。
- 当前 MCP draft/RC 正在把 server-to-client 多轮请求重构为 `InputRequiredResult + requestState`，现在冻结 wire 会把过渡形态写进 Timeline schema。

所以这项能力进入长期 ROADMAP。成熟度证据采用严格版本：有日期、非 draft 的规范；官方 Rust SDK 和至少另一个主流 SDK；两个可互操作客户端；五个独立服务端；真实 Form/URL 场景。即使全部满足，也只说明“可以重新评审”，不会自动启动。

未来若启动，Grow 的映射是：

```text
versioned MCP adapter
  → protocol-neutral PendingInteraction
  → Timeline interaction request/outcome
  → resume the same MCP call
```

它不是新 turn，不进入 FIFO，也不能占用 steer/interjection。UI 的焦点和开关可以是内存状态，但决定 MCP call 是否继续的 request/outcome 必须可恢复。Form 不允许收集秘密；URL 必须展示服务端和完整目标地址、要求显式同意且禁止预取，返回的 token/secret 不进入模型或 Timeline。

协议依据：[MCP Elicitation draft](https://modelcontextprotocol.io/specification/draft/client/elicitation)、[2026 Release Candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)。

### 6.3 Hook：不是只补一个 Prompt gate

Grow 当前 `HookEventName::ALL` 有 15 类事件，`UserPromptSubmit` 仍是 Observe；普通 dispatch 在没有 active handler 时会直接返回。执行结果另外通过 `GrowSessionUpdate::HookExecution` 发给 TUI，并写一份 `HookExecuted` diagnostics。也就是说，当前可以看到一部分 Hook 运行结果，但 Timeline 不能证明某次事件是否触发、哪些 handler 被考虑、为什么没运行、聚合决定是什么。恢复和审计仍然需要猜另一条流。

上游 `UserPromptSubmit` gate 的顺序是先执行 gate，再提交聊天状态；阻断时消息没有持久化，同时还可以把后续 queue 挂在被阻断输入后面。这个行为与 Grow 的事实先行、单一路由不一致，不能照搬。

Grow 的目标顺序是：

```text
InputSubmitted（durable，尚未进入 Surface，也尚未选择路由）
  → HookTriggered
  → HookRunStarted
  → HookRunFinished / HookRunSkipped
  → HookCompleted
  → InputAdmissionResolved
  → Allow：把同一 input_id 交给既有 idle/queue/steer 路由
  → Block：只闭合这次 input admission
```

这里有几个需要钉死的点：

- **所有 Hook event 都进入 Timeline**。不仅是 gate，也包括 Session、Tool、Stop、Notification、Subagent、Compact 等事件。没有 handler、matcher miss、disabled 和 prior block 都要有可解释终态。
- **Trigger 先于副作用**。外部命令或 HTTP 请求开始前先写 `HookTriggered`/run start；结果与聚合决定在受保护行为继续前落账。恢复看到 open run 时记 interrupted/outcome-unknown，不自动重跑可能已经产生副作用的 Hook。
- **结果必须 typed**。至少区分 success、blocked、failed、timed_out、cancelled、skipped；Observe、Prompt、Tool、Stop 的 aggregate decision 不能共用一个布尔字段。
- **数据只保存一次**。已有 tool/message/source 事件用因果引用关联，不能再复制一份 128 KiB 输入和结果。Timeline 保存规范化决策、真正影响控制流的内容和有界诊断；展开 URL、环境变量和秘密不落盘。
- **投影不能反客为主**。TUI annotation、Trajectory 和 diagnostics 都从 Hook Timeline 事件生成。`HookExecution` 可以继续当实时 transport，但不再拥有独立事实。
- **child 自己记账**。子 Agent 内发生的 Hook 写 child Timeline，父 Timeline 只保留原有 spawn/end 与 result 引用。

`UserPromptSubmit` admission 只对真实 `HumanIntent` 有阻断权，包括普通提交、忙碌期间 follow-up 和显式 steer。Goal、Workflow、Notification 等 synthetic origin 仍记录触发与结果，但强制 observe-only。同一 `input_id` 只过一次 Hook；后续排队、提升或重新仲裁不能重复执行。

失败策略只开放给 admission gate：`UserPromptSubmit` 和 `PreToolUse` 可逐 handler 设置 `on_failure = allow | block`，默认 allow。Observe、Stop、SubagentStop 使用现有语义，在这些事件上配置 `on_failure` 应直接报错，不能静默忽略。首次明确 block 后停止执行后续 handler，并把剩余项记成 `skipped(prior_block)`。

block 不冻结全局队列，也不创建一个空 turn。被拒绝输入仍在 Timeline 和用户 transcript/Trajectory 中可见，但不进入模型 Surface；当前 active turn 和其他 FIFO 输入仍由原来的 owner 仲裁。

### 6.4 采样、截断和恢复

上游这一轮把“传输为什么结束”“哪些输出已经完整”“恢复时能否安全继续”拆得更细。Grow 已经有 typed truncation、context overflow、pause-turn 和 incomplete block 处理，所以重点不是移植代码，而是用同一张表证明边界没有漂移。

| 场景 | 上游经验 | Grow 结论 |
|---|---|---|
| `Length + text` | 保留可用文本并 continuation | 已有；继续沿同一 turn lineage，补回归锁定 |
| incomplete reasoning | 不作为下一次请求的完整 carrier | 已有；丢弃未完成块 |
| incomplete tool call | 不执行 | 已有；不能持久化为 executable call |
| complete tool call + `Length` | 完整调用优先 | 已有；补恰好一次执行测试 |
| context overflow | 与普通 Length 分开 | 已有；compact 后仍超限则 typed terminal |
| `pause_turn` | 保存完整 assistant，再无 prompt resample | 已有；不得伪造用户消息 |
| provider limit reason | 原始 stop reason 仍有诊断价值 | 吸收；保留 raw reason，再映射 Grow typed class，避免统一折成 `Length` |
| transient sampler failure | 同一请求内有限重试 | 审计；只有无不可逆输出或可证明安全恢复时才重试 |
| Empty response | 不与 truncation 混为一类 | 保持现有路径，单独审计，不借本轮改变策略 |
| cancel / late result | 竞争路径只能有一个终态 | 补竞态测试；迟到结果不能复活或复制 turn |
| crash restore | 从已提交事实恢复 | Timeline 重放，不重复 tool/output，保持原 causal identity |
| model family switch | 上游会主动 compact | 不吸收；只有证明 carrier 不兼容后才走现有投影/压缩机制 |

对照表会进入 `truncation-recovery.md`。若一行已经有实现和测试，只标记 covered；发现缺口则单独拆任务，禁止趁审计重写整个 sampler/recovery。

### 6.5 active subagent follow-up

上游新增了向活跃 child 发送 follow-up 的能力，并显式区分 Accepted、NotOwned、NotActive、Saturated、AdmissionUncertain、Deadline、Limit 和 ChannelClosed。值得吸收的是它对“发送成功”这个词的收紧：写进 channel 不等于 child 已经接纳；不确定也不等于拒绝。

Grow 不移植上游工具和通道，只审计以下契约：

- ownership 与 liveness 分开；
- Accepted 只在消息原子进入 child 的既有 admission/Timeline 后成立；
- 使用稳定 message id，区分 accepted、rejected、unconfirmed；
- payload、in-flight、deadline、closed 使用 typed outcome；
- 不增加全局通道，不复用父 Agent FIFO。

当前架构若不满足，只形成独立设计债务。本轮不会因为一份对照评审就实现新的 follow-up feature，也不会照抄上游的固定大小上限。

### 6.6 status line 与 Worktree

上游 `[ui.status_line]` 已由用户文档和布局代码交叉确认：它是 pager 底部的附加行，全屏在 shortcuts bar 上方，minimal 在 prompt info row 下方。它不是 Grow 当前顶部的 `AgentStatusBar`，也不是运行时 `TurnStatus`。

用户已经决定搁置，所以不吸收 builtin items，不讨论 command runner，也不改 UI/config。这里只保留位置判断，避免未来再次把两个 status 概念混在一起。

Worktree pool、复用与 trust 边界在这 11 个提交中仍有反复调整。由于它不是 Grow 当前关键路径，本轮只放入长期观察，不产生实现或测试任务。

### 6.7 安全经验：转成威胁模型，不换权限模型

这一轮值得保留的不是具体 patch，而是跨阶段、跨身份和跨 generation 的失配风险：

- sandboxed 阶段写出的 Hook/config 不能在后续 unsandboxed consumer 中自动升级为可执行输入；
- 对当前仓库的信任不能传递到后来 clone、替换或新建的目录；
- deployment/auth credential 必须绑定预期 server identity 与目标 authority；
- MCP permission、init failure 与 reconnect 状态必须绑定 server identity、transport 和 config generation，旧 episode 不能污染新实例；
- 低权配置不能关闭高权策略，但 Grow 只验证自己的 provenance hierarchy，不增加上游 managed-hook 层；
- sandbox、quota 和 network denial 要保留 typed terminal，避免被采样层误判成可重试错误；
- socket mask、异步 I/O 等绕过面进入威胁测试，但不因此同步上游 sandbox architecture。

这些项目进入 v2.1.0 临时待办。真正执行时仍按 Grow 当前 descriptor RWX、exact-identity grant、一次性 permit 和 Timeline owner 验收，不能混出第二套权限事实。

### 6.8 任务归档

- MCP Elicitation、Worktree 与底部 status line 只进入长期 ROADMAP。
- Hook Timeline/admission、恢复对照、subagent 契约审计和安全回归进入 `TODO-v2.1.0.md`，状态为 `Waiting / manual-start-only`。
- 原 ROADMAP 中的 Behavior 与 Workflow 架构债务一并迁入 `TODO-v2.1.0.md`，让 ROADMAP 只保留长期方向。
- 临时 TODO 全部完成后，稳定契约回写对应架构文档，然后删除该文件；评审文档只保留当时的取舍依据，不成为实现事实源。
