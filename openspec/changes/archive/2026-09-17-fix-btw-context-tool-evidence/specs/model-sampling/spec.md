## ADDED Requirements

### Requirement: Side questions preserve completed tool evidence from their frozen context

`/btw` SHALL 从一次冻结的已提交主会话上下文构造只读请求，保留其中身份有效、可无歧义配对的已完成工具调用及结果、结果附件和普通 Assistant 正文，包括连续工具交换构成的尾部。未完成调用 SHALL NOT 输出悬空工具协议或伪造执行结果，同批已完成调用 SHALL 保留。Sideband SHALL 不提供工具能力、不改变主模型 Surface，重试 SHALL 复用同一冻结输入。

#### Scenario: Completed tools are the latest context
- **WHEN** `/btw` 冻结上下文的尾部由连续已完成工具交换组成，尚无单独的 Assistant 总结
- **THEN** 发往 provider 的请求包含这些调用、结果和附带正文，随后才是旁路问题。

#### Scenario: Only some parallel calls have completed
- **WHEN** 最新 Assistant 发出多个调用，冻结时只有部分结果已提交
- **THEN** 请求保留已完成调用及对应结果和 Assistant 正文，不包含无结果调用的工具协议，之前的完整交换保持。

#### Scenario: Latest call is still running
- **WHEN** 冻结上下文以未得到任何结果的调用结束
- **THEN** 请求保留其普通 Assistant 正文及之前的完整交换，不声称该调用已完成，也不改变主上下文中的运行中调用。

#### Scenario: Main conversation advances during a retry
- **WHEN** 第一次旁路请求 overload 后，主会话提交了新消息，再发生旁路重试
- **THEN** 重试仍使用首次冻结的内容及来源引用，旁路问题与回答均不追加到主模型 Surface，且没有工具定义或 tool choice。

实现与验证入口：`crates/codegen/shell/src/session/actor/recap.rs` 的 `handle_side_question`；`crates/codegen/shell/src/session/actor/tests/recap_display_only_tests.rs` 的 Sideband wire 回归。
