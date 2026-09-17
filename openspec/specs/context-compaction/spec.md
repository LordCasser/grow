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
