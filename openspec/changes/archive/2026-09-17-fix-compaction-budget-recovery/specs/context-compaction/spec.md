## ADDED Requirements

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
