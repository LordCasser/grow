## Context

切换模型由 SessionActor 在 Step 边界应用，并通过 `ReplaceSamplingRoute` 开启新的 continuation epoch。此前已将 portable 工具往返改为结构化协议，同时规定删除旧 provider reasoning。DeepSeek Responses 的无状态 thinking 协议却要求工具后续请求包含先前 reasoning item；因此“删除 reasoning + 保留 function_call”在该目标端点不是合法 portable 输入。

## Goals / Non-Goals

目标是在不恢复 source provider opaque continuation 的前提下，让已明确声明该要求的 Responses 路由重建可接受请求，并让恢复共享当前 logical sampling 的 attempt/deadline。非目标是按模型名硬编码 provider 能力、默认向所有端点暴露 reasoning、持久化供应商兼容状态，或改变切换/工具调度边界。

## Decisions

1. **错误边界只分类明确协议拒绝。** `SamplingError` 在 400 API 错误同时包含 `reasoning_text`、thinking mode 与 passed back 语义时产生 `portable_reasoning_required` 类型化事实；DTO 往返保留该字段。SessionActor 只消费字段，不再次匹配错误文本。
2. **兼容状态由 ChatState 按当前 route 持有。** 首次拒绝通过有确认命令开启当前 route 的 Responses portable reasoning 回放。native reset、投影重对齐和 compaction 保留该路由能力；真正 `ReplaceSamplingRoute` 清除它，避免对其他端点泄漏。
3. **只回放已有可见 reasoning。** portable prefix 中的 `ConversationItem::Reasoning` 被编码为无 id/status/encrypted_content 的 Responses reasoning item，正文来自既有中性可见文本。Chat Completions、Messages 与未学习的 Responses 路由继续删除它；有效 native span 继续使用原生内容且不重复。
4. **恢复单次启用、有界重试。** 命令仅在当前 Surface 确有非空 reasoning 且模式尚未启用时返回 changed。SessionActor 只有 changed 时重提交；后续同类拒绝不重新开启预算或形成无限循环。

## Risks / Trade-offs

- 这是由端点明确拒绝触发的兼容学习，首个请求仍会消耗一次 attempt；相比模型名或 base URL 启发式，它不会把供应商能力固化进路由层。
- visible reasoning 可能不是 source provider 的完整 opaque continuation，但目标协议要求的是可序列化 reasoning text；回放范围仅限已经进入 Timeline Surface 的文本，不合成内容。
- 能力不持久化，进程恢复或再次切换到同一路由后可能重新学习一次；这保持状态与 ephemeral continuation 同域，避免新增持久化实体。
