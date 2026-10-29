# context-compaction Specification

## Purpose
定义压缩对模型可见上下文的影响。约束选定 Surface 范围的替换、后台生成与前台发布的边界，以及取消后迟到结果的处理，避免异步压缩覆盖新的上下文事实。

## Requirements

### Requirement: Range scoped compaction
压缩 SHALL 只替换选定 Surface 范围，保留未选中内容的 identity。

#### Scenario: 局部压缩
- **WHEN** 只压缩部分上下文
- **THEN** 未选中 Surface identity 保持不变。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `partial_compaction_preserves_unselected_surface_identity`。

### Requirement: Boundary publication
异步压缩 SHALL 冻结生成输入，并由前台在闭合 Step 边界发布结果。

#### Scenario: 前台仍在执行
- **WHEN** 后台压缩结果已生成而 Step 尚未闭合
- **THEN** 后台不能自行改写当前 Surface；边界提交重新检查 authority 和 model。

证据：`crates/codegen/shell/src/session/actor/compaction.rs` — `PreparedCompaction`。

### Requirement: Late result invalidation
取消后的异步压缩 SHALL 丢弃迟到的 provider 结果。

#### Scenario: 取消后才收到结果
- **WHEN** 压缩任务已被控制转换取消
- **THEN** 迟到结果不能提交到上下文。

证据：`crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs` — `async_compaction_cancel_discards_late_provider_result`。

### Requirement: Compaction summaries describe only replaced history
压缩摘要的模型可见载体 SHALL 明确摘要只描述被替换的历史，摘要之后保留的消息是更新的任务和执行进度。旧摘要中的完成、等待用户或下一步建议 SHALL NOT 被载体描述为当前回合的权威状态；压缩本身 SHALL NOT 改变用户授权范围。

#### Scenario: Old completed work precedes a newer active task
- **WHEN** 被压缩片段中的任务已完成，但保留 tail 包含更新的用户任务和工具结果
- **THEN** 模型请求保留该 tail 的顺序，并包含明确以较新消息判断当前任务的摘要范围说明。

验证入口：`crates/codegen/shell/src/session/actor/compaction.rs` 的摘要载体组装；`crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs` 的异步场景组。

### Requirement: Async compaction hands off to an admitted next step
前台在闭合 Step 边界成功发布异步压缩，并获准开始同一普通 turn 的后继 Step 时，SHALL 在未替换 tail 之后、下一 Step 请求之前持久化恰好一个 synthetic `AutoContinue` 提示，说明压缩不等于任务完成并要求继续最新授权的未完成工作。该提示 SHALL NOT 成为真实用户输入、权限证据或新的 turn，也 SHALL NOT 单独触发额外采样。

#### Scenario: Continue after a tool result
- **WHEN** 工具 Step 已结束、异步压缩已提交且当前 owner 允许下一 Step
- **THEN** 下一请求包含旧片段摘要、保留的工具事实和其后的续接提示；重复构造请求不重复追加提示。

#### Scenario: No next step is admitted
- **WHEN** 压缩仅生成尚未提交、失败或取消，或前台已完成/失去 owner/被控制终止
- **THEN** 不追加该续接提示，也不为了压缩打开新 Step 或 turn。

#### Scenario: Ordinary final response after handoff
- **WHEN** 带有续接提示的请求收到完整、合法且无工具调用的最终回复
- **THEN** 回合仍按正常完成路径结束，不将该回复改判为流失败或无条件重新采样。

#### Scenario: Background result survives into a new turn
- **WHEN** 后台摘要在新 turn 的首个 Step 前提交
- **THEN** 新输入自身承担任务接纳，不追加旧回合的续接提示；摘要范围说明仍保留。

#### Scenario: Continuation persistence fails
- **WHEN** 续接提示未取得持久化确认
- **THEN** 不开始下一 Step 的 provider 请求，按现有错误路径收尾。

验证入口：`crates/codegen/shell/src/session/actor/turn/mod.rs` 的 between-step 准入；`async_compaction_between_steps_continues_the_latest_task_once`、异步取消/失败/跨回合场景；ChatState durable append 故障测试。

### Requirement: Compaction retains structured completed tool history

局部压缩保留 tail 中完整的本地工具往返时，后续请求 SHALL 保留其结构化调用/结果及顺序，同时继续撤销旧 native reasoning 和签名。摘要范围说明及已获准下一 Step 的 AutoContinue 规则保持；压缩 SHALL NOT 重新执行历史工具。

#### Scenario: Async publication retains a recent tool result
- **WHEN** 工具执行后异步摘要在 Step 边界发布，完整工具往返留在 tail
- **THEN** 下一请求仍有配对工具协议、摘要范围说明及应有的 AutoContinue；本次工具只执行一次。

#### Scenario: Surface replacement and replay
- **WHEN** 压缩范围替换完成或随后恢复会话
- **THEN** 保留的工具事实从 Timeline 以结构化中性历史投影，不从旧 artifact 或相同模型名恢复 native。

### Requirement: Compaction can split the prompt segment retained for its tail budget
整 prompt 选区无法提供足量摘要来源时，压缩 SHALL 从为保留预算选中的最早 prompt 之后寻找完整响应组边界，即使其后还有更新的 prompt。选区 SHALL 保留该 prompt、至少既定预算的原文 suffix、后续 prompt 顺序和完整工具往返，不降低最小来源阈值。

#### Scenario: Short newer prompt follows a long segment
- **WHEN** 最新 prompt 的 suffix 小于保留预算，更早长片段内存在同时满足来源阈值和保留预算的完整响应组边界
- **THEN** 从更早片段内选择可压缩范围，保留其 prompt 及边界后的全部内容，不因只检查最新 prompt 而失败。

#### Scenario: Retained boundary contains reasoning and tools
- **WHEN** 边界后的响应组含 reasoning、assistant 工具调用和结果
- **THEN** 整组在 tail 中保持原有顺序及身份，不从 reasoning 中部或工具结果开始 tail。

#### Scenario: No sufficient safe range exists
- **WHEN** 保留预算或最小来源阈值无法在任何合法边界同时满足
- **THEN** 不创建压缩计划，不以缩短必需 tail 或拆开工具往返规避失败。

### Requirement: Manual compaction RPC preserves authoritative error outcomes
手动压缩扩展接口 SHALL 保留 actor 错误的 ACP code 与结构化 data，包括已发布终态标记。Pager 收到已发布失败终态的错误 SHALL NOT 重新展示“结果未知”或重复终态；没有权威终态的传输失败仍使用未知结果反馈。

#### Scenario: Published compaction failure returns through RPC
- **WHEN** actor 已发布压缩失败通知并返回带终态标记的 ACP 错误
- **THEN** RPC 原样透传该错误，客户端结束等待并只保留已有失败通知。

#### Scenario: Unmarked error or missing response
- **WHEN** actor 返回无终态标记的错误，或响应通道关闭
- **THEN** RPC 保留前者 code/data，后者报告接收失败；不伪造已发布终态。

### Requirement: Summary requests budget the complete bounded input and output
压缩 SHALL 显式约束摘要输出 token 上限，并从模型窗口为输出、摘要指令和估算误差预留空间。Sideband attempt 记录的输出上限 SHALL 与实际 provider 请求一致。

#### Scenario: Near-window context with a large model output default
- **WHEN** 主模型配置的大输出上限与摘要输入共同超过窗口
- **THEN** 摘要发送独立的有界输出上限，完整请求输入处于其预留后的预算内，不继承过大的主模型默认输出上限。

### Requirement: Bounded summary sources match the replaced range
压缩因预算或明确上下文超长缩小输入时 SHALL 在完整历史边界缩小真实选区，并保留该选区的已完成工具调用参数、结果及附件。成功摘要 SHALL 只替换实际摘要输入覆盖的 Surface IDs；未纳入输入的内容 SHALL 保持原身份，不以工具名代替工具结果。普通暂态重试 SHALL 复用冻结输入。

#### Scenario: Provider rejects an estimated fitting request
- **WHEN** provider 明确拒绝摘要输入超长而更小的合法选区仍满足最小来源阈值
- **THEN** 有界重试缩小同一冻结快照的完整选区，Sideband manifest、摘要输入与成功 replacement target 一致，剩余历史完整保留。

#### Scenario: Completion exists only in tool output
- **WHEN** 旧计划称尚未执行而已完成工具结果给出成功证据
- **THEN** 所选区内的该结果正文和附件进入摘要请求，non-verbatim 模式也保留证据。

#### Scenario: No bounded complete source exists
- **WHEN** 输入预算无法容纳满足最小来源阈值的完整选区
- **THEN** 本次压缩明确失败且不替换历史，不截断结果或把未摘要的历史纳入 target。

#### Scenario: Candidate range ends in an unfinished exchange
- **WHEN** 初始候选范围含未完成工具调用、部分结果批次或没有后续响应的 reasoning 尾部
- **THEN** 完整选区截止到此前合法边界，未完成部分保持在 target 之外；不足最小来源时明确失败，不将输入预处理会丢弃的尾部计入 replacement。

### Requirement: Size failures do not permanently disable session recovery
摘要输入超长或提交后仍超窗 SHALL 有界结束当前恢复；其自动压缩抑制 SHALL 只作用于当前 turn，后续真实 turn 和手动压缩可重新尝试。持久化错误仍遵循现有 fail-closed 边界。

#### Scenario: Failed summary followed by a new user turn
- **WHEN** 摘要因输入大小失败，随后用户在同一 session 提交新 turn 且 provider 恢复可用
- **THEN** 自动压缩可重新准入并继续正常采样，不要求创建新 session。

#### Scenario: Replacement still leaves context over the window
- **WHEN** 摘要已提交但投影仍超窗
- **THEN** 当前 turn 有界失败且压缩事务保持 Completed，下一真实 turn 不继承永久大小抑制。

### Requirement: Async compaction completion reports a materialized request projection

异步压缩的用户可见完成通知 SHALL 等下一次普通模型请求完成 request projection 后再发布，并以该投影作为 `tokens_after`。Surface replacement 后、请求重新物化前的中间估算 SHALL NOT 作为异步压缩最终值展示。每次成功提交最多发布一个完成通知；同步提升、同步自动压缩和手动压缩保留各自的即时完成语义。

#### Scenario: Between-step request materializes after async commit

- **WHEN** 异步压缩在闭合 Step 边界成功提交，随后普通后继请求完成组装
- **THEN** 提交与请求组装之间没有异步完成通知；组装后发布的 `tokens_after` 等于该请求已应用的 ChatState projection。

#### Scenario: Completed turn has no next request yet

- **WHEN** 异步压缩在已结束 turn 的边界提交，尚无后继普通请求
- **THEN** 不展示 Surface-only 中间值，完成通知保持待发布，且其间不启动另一轮后台压缩。

#### Scenario: A later turn materializes the next request

- **WHEN** 待发布通知存在，后续 turn 的首个普通请求完成 projection
- **THEN** 使用该 projection 发布恰好一次异步完成通知；后续请求构建或边界检查不重复发布。

#### Scenario: Request construction fails before projection

- **WHEN** 下一普通请求未能完成构建和 request projection
- **THEN** 不发布带猜测 `tokens_after` 的完成通知，也不消费待发布记录。

#### Scenario: A later replacement supersedes the pending display

- **WHEN** 待发布期间同步压缩、手动压缩或 rewind 在下一普通请求前替换上下文
- **THEN** 丢弃旧异步压缩的待发布通知，不把新 replacement 的 token 数归因给旧异步事务。

#### Scenario: Route changes while an async completion awaits projection
- **WHEN** 异步 replacement 已提交且完成通知待发布，其后仅切换模型路由并成功物化下一普通请求
- **THEN** 不提前发布；通知采用该新请求投影并且只发布一次；若切换触发新的 replacement，旧通知按既有 supersession 规则丢弃。

### Requirement: Cancellation preserves and drains the compaction generation owner
When ordinary authority invalidation cancels a background compaction generation, the Session SHALL signal cancellation without joining an unfinished worker from inside the root command handler, and SHALL retain the worker in its existing owner slot. An explicit suspension or teardown barrier SHALL keep the root accounting inbox able to process the worker's Goal settlement receipt while it joins the worker; it SHALL publish the cancellation terminal only after that drain. A runtime freeze SHALL use the existing shared deadline for this drain and SHALL fail without claiming a settled checkpoint when the worker has not joined by that deadline. Cancelled or late generation output SHALL NOT be committed to the visible Surface.

#### Scenario: Goal usage invalidation and Sideband settlement share the root inbox
- **WHEN** a promoted summary is still running and `RecordGoalUsageIncomplete` is processed by the root accounting inbox
- **THEN** invalidation signals cancellation and returns from the current inbox command, allowing the same inbox to process the worker's `SettleGoalUsageAttempt` before a later explicit drain records cancellation.

#### Scenario: Promoted generation is cancelled after incomplete Goal usage
- **WHEN** a promoted summary is still running as the Goal usage window becomes incomplete
- **THEN** the same worker is cancelled and joined, its Sideband attempt reaches a terminal settlement, the Goal settlement is not detached, and the generated summary is not committed.

#### Scenario: Suspension deadline expires during generation drain
- **WHEN** runtime suspension cancels a background summary but its owner has not joined by the shared deadline
- **THEN** suspension returns an unconfirmed result and retains the worker owner for retry; it does not publish a completed frozen checkpoint.

#### Scenario: Teardown drains with the accounting inbox active
- **WHEN** graceful or fatal teardown has an unfinished cancelled compaction worker
- **THEN** its existing drain loop continues dispatching Goal usage and settlement commands until the compaction worker and non-Workflow children finish, or teardown fails without publishing a final persistence frontier.

#### Scenario: Manual compaction waits for the existing generation owner
- **WHEN** a manual compaction starts while an asynchronous summary still owns the compaction lease
- **THEN** manual compaction signals cancellation, leaves the root Goal inbox available while it joins that worker, and acquires the lease only after the original worker has settled.

#### Scenario: Ordinary stop precedes a late background summary
- **WHEN** 未提升的后台摘要仍在等待 provider，普通取消已返回，然后 provider 返回有效摘要
- **THEN** 后续发布边界丢弃迟到结果，不提交 replacement；排空仍由现有显式 drain 边界负责。

#### Scenario: Promotion waiter is dropped and the same generation is awaited again
- **WHEN** 后台 generation 已经 durable 提升，但等待方 future 被丢弃，随后前台再次等待同一 worker
- **THEN** 复用已有 promotion 事实和 provider attempt，保留原截止时间，并且最多提交一次 replacement；generation token 真正取消时不得发布迟到结果。

#### Scenario: Rejoined promotion still waits for the original durable acknowledgement
- **WHEN** 原 Promoted 写入尚未确认，等待方退出后另一等待方提交同一 promotion
- **THEN** 后者等待原 durable 边界；原 ACK 确认后复用同一事件，原写入失败或超时则拒绝后续写入，不能因事件已保留而提前报告成功。

### Requirement: Planning context projects current local obligations

模型上下文 SHALL 从权威投影提供当前局部计划、直接相关变化、有效等待与未验收交付的有界摘要及必要引用，不在每轮复制整棵 Agent 树。完成细节 MAY 压缩；继承 transcript 或旧 summary SHALL NOT 覆盖当前 owner 和计划。压缩自身 SHALL NOT 触发额外模型续跑。Pager 收到 compaction 完成通知 SHALL 保留当前 TODO 投影；只有明确的 Planning/Plan 更新可替换该投影。

#### Scenario: CC-01 Large child tree has a small local context
- **WHEN** root 有多个 child 和大量已完成步骤，当前只等待一个必需结果
- **THEN** 当前上下文保留该等待、相关局部计划和结果引用，其他已完成细节可摘要；不因压缩自动规划或加载整个后代列表。

#### Scenario: CC-02 Inherited history mentions a parent plan
- **WHEN** child 的 fork 历史或压缩摘要含父 TODO，child 已有自己的当前计划
- **THEN** 历史保留为来源，模型得到明确的当前 child 投影；旧文本不成为可写父计划或当前列表。

#### Scenario: Compaction completion preserves the visible local plan
- **WHEN** root 或 child 收到同步或异步 compaction 完成通知，且当前 session 已有 TODO 投影
- **THEN** compaction 只更新自身状态与用量，现有 TODO 保持可见；之后收到明确的 Planning/Plan 更新时仍按该更新刷新列表。

### Requirement: Compaction cannot acknowledge or erase pending replanning

压缩发布及下一安全执行边界 SHALL 重新核对当前计划和未闭合责任。并发到达的重规划、Consumed 但未 Handled 的变化及新结果 SHALL 保留；压缩摘要 SHALL NOT 冒充处理确认或重新消费旧消息。

#### Scenario: CC-03 Parent update arrives during compaction
- **WHEN** 压缩开始后父项变化或 child 结果到达
- **THEN** 发布后的当前投影仍含新责任，旧摘要不能覆盖它，也不能将其标为已处理。

#### Scenario: CC-04 Consumed update survives a compacted history
- **WHEN** 已消费的重规划消息进入被压缩范围，但 child 尚未提交处理确认
- **THEN** 未处理引用和所需要求仍可获得；同一历史消息不重复注入，处理确认只能由合法 child 计划命令产生。

证据与实施入口：`crates/codegen/shell/src/session/actor/compaction.rs` 的 boundary publication、`crates/codegen/chat-state/src/timeline.rs` 的通知与 context identity。

### Requirement: Fresh skill results remain complete until their first consuming step settles
显式技能结果从上下文发布到首次关联模型步骤获得已接纳响应之间，SHALL 作为完整必需内容保留。pre-prune、前后台压缩与请求重建 SHALL 不裁剪、摘要替换或丢弃这些结果；用户来源的所属输入 SHALL 与结果保持关联。该保护 SHALL 基于已有 Surface identity 与请求 / 响应事实，首次消费完成或执行明确终止并结算后解除，不形成永久技能状态。

#### Scenario: Pre-prune selects a fresh oversized result
- **WHEN** 新技能结果超过普通单项裁剪预算，但尚未获得首次关联模型响应
- **THEN** pre-prune 保留该结果全文，只能选择其他可裁剪历史；预算无法满足时明确失败，不带截断结果继续采样。

#### Scenario: Async compaction publication overlaps fresh content
- **WHEN** 后台压缩准备后，新加载内容进入 Surface，发布时选区与尚未首次消费的内容冲突
- **THEN** 在现有 authority / revision 边界重新校验并拒绝冲突发布，不覆盖新输入或技能正文。

#### Scenario: First consuming step completes
- **WHEN** 包含完整加载内容的请求获得已接纳响应
- **THEN** 该加载结果可按普通历史参与后续压缩，历史 Loaded 不承诺正文常驻。

#### Scenario: Execution ends before consumption
- **WHEN** 首次消费前执行明确终止或取消
- **THEN** 先确认撤下用户组或为模型结果提交配对的交付错误，再解除保护；原始历史保留，后续独立请求不重新激活已终止内容。

#### Scenario: Cold interruption closes an unconsumed execution
- **WHEN** 技能组已持久化而首次响应尚未接纳，进程退出且没有有效的挂起 checkpoint
- **THEN** 普通中断恢复在关闭原 Turn 前结算未消费正文，保留原输入和 Hook 身份，不让正文进入下一独立 Turn；有效挂起仍保留原 Turn 的正文和首次消费保护。

证据入口：`crates/common/compaction/src/prune.rs` 的 plan_tool_result_pruning、`crates/codegen/shell/src/session/actor/compaction.rs` 的 maybe_pre_prune_at / 后台发布，以及 `crates/codegen/chat-state/src/actor/mutations.rs` 的 Surface 修改。本条只增加首次消费所需的短期约束。

### Requirement: Compaction completion and admission use effective context pressure
Manual and synchronous compaction completion and post-replacement window checks SHALL use the current effective request projection with known request envelope and provider calibration, never a raw Surface delta intermediate. A subsequent full request SHALL refresh its current envelope and pass the independent admission check. Background completion SHALL retain its existing deferred request-projection semantics.

#### Scenario: Retained reasoning after manual replacement
- **WHEN** a manual summary replaces a range while raw reasoning remains in history
- **THEN** completion reports nonempty effective retained content accurately and repeated request assembly does not introduce phantom growth.

### Requirement: Every foreground compaction start has a terminal presentation
Every published foreground or promoted compaction Started SHALL receive one terminal presentation after the transaction settles. A committed replacement SHALL be presented as completed even if a post-commit error fails its enclosing turn; failures before commit and cancellation SHALL use their corresponding terminal. A new committed replacement SHALL invalidate an older pending asynchronous completion notice regardless of the later enclosing result.

#### Scenario: Committed replacement remains over window
- **WHEN** replacement commits and convergence validation fails the turn
- **THEN** Timeline and presentation both settle compaction, the turn error propagates, and the UI does not remain compacting.

#### Scenario: Replacement supersedes deferred completion then fails
- **WHEN** a pending async notice exists and a manual or synchronous replacement commits but later returns an error
- **THEN** the older notice is discarded and cannot attribute the new request size to the old transaction.

#### Scenario: Summary generation is cancelled
- **WHEN** a foreground or promoted summary generation is cancelled before replacement
- **THEN** its foreground owner publishes exactly one Cancelled terminal, without a second terminal from the generation layer.

#### Scenario: Pruning loses persistence acknowledgement
- **WHEN** pre-prune cannot confirm its write or the ChatState owner is unavailable
- **THEN** the foreground compaction settles with failure and does not fall through to a summary provider request; a plan rejected before writing may still use the normal summary path.

### Requirement: Queued manual compaction yields at a closed execution boundary
普通 turn SHALL 在已排队手动压缩需要取得前台 owner 时，于闭合 Step 边界结束当前执行并交给既有 idle arbitration；被跳过的自动压缩 SHALL NOT 被当作恢复进展而无限重建请求。

#### Scenario: Manual request competes with automatic pressure recovery
- **WHEN** 手动压缩已排队且普通请求投影超过自动压缩阈值
- **THEN** 普通 turn 有界让出 owner，手动请求仍保留并可运行，不反复生成空 Step 或向超窗 provider 发请求。

### Requirement: Summary source fitting uses the transmitted history projection
摘要来源预算 SHALL 使用将实际发送的历史投影，同时保持原始 Surface 的完整选区边界及 target identity。不会发送的 reasoning 和回忆派生内容 SHALL NOT 挤占来源预算；重试缩小选区也使用同一投影。

#### Scenario: Omitted reasoning dominates raw history
- **WHEN** 原历史包含大量在当前摘要协议中不发送的 reasoning，而完整来源投影可装入预算
- **THEN** 接纳该合法来源，不因原始 reasoning 体积拒绝压缩；replacement 仍只覆盖选定完整范围。

### Requirement: Only completed summary responses may replace history
压缩 SHALL 仅接纳成功结束的摘要响应作为历史 replacement。非成功终态即使有足量正文，也 SHALL 按现有失败/有界恢复路径处理，不提交部分摘要。

#### Scenario: Provider returns partial or refused summary text
- **WHEN** 摘要流以输出截断、上下文超窗、拒绝或暂停终止，并附带非空正文
- **THEN** 不以该正文替换历史；Sideband 和压缩事务按实际结果结算。

#### Scenario: Provider stream ends without a completion terminal
- **WHEN** provider stream emits summary text but reaches EOF without a typed successful completion terminal
- **THEN** the response is rejected through the bounded failure path and its text cannot replace history.

### Requirement: Summary cleanup preserves quoted control tokens as evidence
摘要清理 SHALL 只剥离明确的前导 scratchpad，保留正文引用控制标签之前和之后的事实，不以是否数字开头判断正文可删除。

#### Scenario: Prose summary quotes an analysis closing tag
- **WHEN** 摘要正文以自然语言开始并在后文引用 analysis 结束标签
- **THEN** 标签可被中和，但正文前面的证据保持完整。

### Requirement: Compaction persistence uncertainty preserves committed boundaries
压缩在 replacement、终态或必要控制修复的 durable ACK 不明时 SHALL 返回错误并阻断依赖采样。已确认 replacement SHALL 保持其 identity；冷恢复 SHALL 依据实际持久事实闭合事务，不重复替换，也不把未确认的写入作为 live 成功依据。

#### Scenario: Replacement or terminal acknowledgement is lost
- **WHEN** replacement 或 Completed 记录的 durable ACK 丢失且既有精确重试仍无法重新确认，记录可能已经持久化
- **THEN** live 调用不能报告整体执行成功；恢复按实际记录保留原历史或确认后的 replacement，且每个事务最多有一次有效替换及一个终态。

#### Scenario: Required control repair fails after replacement
- **WHEN** replacement 已确认但其后的必要控制上下文重投影未获得持久确认
- **THEN** 保留已提交 replacement，当前执行失败并阻断下一请求；事务按已提交事实闭合，不能被改判为未提交失败。

#### Scenario: Lost acknowledgement is recovered by an exact retry
- **WHEN** 记录实际已持久化但第一次 ACK 丢失，既有写入循环对同一事件重试并获得确认
- **THEN** 复用同一 seq 和 payload，只应用一次逻辑事实；不重发模型请求、不重复替换或计费。

#### Scenario: Writer process dies after a durable replacement but before acknowledgement
- **WHEN** replacement 已经通过真实文件与目录同步，返回原 actor 的 ACK 未确认时 writer 进程被强制终止
- **THEN** 新进程取得独占 writer 后保留同一 replacement 与未选中的 Surface 身份，恢复必要 Control 上下文并且只闭合一个 Completed，不重跑摘要生成。

#### Scenario: Recovery process dies after committing its terminal
- **WHEN** 恢复进程写入 Completed 后、其 ACK 返回前再次中断
- **THEN** 下一独立恢复进程承认已有终态，保持原日志 prefix，不追加重复 replacement、terminal 或 Control 重投影。

#### Scenario: Failed summary-only recovery is loaded again
- **WHEN** 摘要事实已持久化但 replacement 未提交，恢复以 Failed 闭合该事务，随后再次启动
- **THEN** 校验接受该合法失败事务，保留原历史及摘要来源证据，不要求失败事务存在 replacement。

#### Scenario: A failed range is summarized by a later transaction
- **WHEN** 含 Summary 的失败事务未替换历史，后续新事务重新压缩相同 SurfaceRange 并提交不同摘要
- **THEN** 每个 Summary 只与自身事务中的 replacement 对账，不借用另一事务的 replacement；原失败事实仍可重复加载。
