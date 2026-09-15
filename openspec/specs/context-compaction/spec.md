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
