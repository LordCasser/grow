## ADDED Requirements

### Requirement: Portable Responses history retains output message phases

来自 Responses 的 Assistant 文本 SHALL 在中性历史中保留逐条输出消息边界和 `commentary`/`final_answer` 阶段，不保留原生输出消息 ID 或状态。构造 portable Responses 请求时 SHALL 按原消息顺序及阶段投影，切换到 Chat Completions 或 Messages 时继续使用扁平正文。已有本地工具调用/结果配对和 native continuation 规则保持有效。

#### Scenario: Portable Responses phase after recovery or route switch
- **WHEN** accepted Responses output has multiple assistant messages with different phases and its native continuation is unavailable
- **THEN** durable neutral history retains their text boundaries and phases, and a subsequent Responses request emits separate assistant input messages in order with those phases.

#### Scenario: Local tool exchange follows phased messages
- **WHEN** one Responses output also contains local function calls and the corresponding results have been admitted
- **THEN** portable requests keep the complete call/result batch paired exactly once after the phased assistant messages.

#### Scenario: Legacy or transformed Assistant text
- **WHEN** an Assistant record has no boundaries or its text has been redacted/truncated so stored ranges cannot describe it
- **THEN** Responses request projection emits the existing single phase-less text message, without slicing invalid offsets or inventing a phase.

#### Scenario: Compaction replaces source messages
- **WHEN** compaction replaces a source range with a summary
- **THEN** exact phase replay is retained for un-compacted Assistant items, while the summary makes no claim to preserve discarded messages' phases.
