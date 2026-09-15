## ADDED Requirements

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
