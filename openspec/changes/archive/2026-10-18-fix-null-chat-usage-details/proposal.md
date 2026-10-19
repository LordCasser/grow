## Why

Chat Completions usage 尾帧允许可选 token 明细为 `null`。Grow 当前将这些计数反序列化为带 `serde(default)` 的 `u32`，显式 `null` 因而触发 decode error。若 `finish_reason: tool_calls` 已到达，仍会因为尾帧解码失败而使整次响应失败，完整响应和用量不能接纳、结算。

已确认的本机证据：GameNow session `01a14e30-5417-7c61-b7c0-8b3412a9289d` 的 Timeline seq 1718/1737 所引用原始 Chat SSE usage 尾帧，含 `prompt_tokens_details.audio_tokens: null`，以及 `completion_tokens_details.accepted_prediction_tokens`、`audio_tokens`、`rejected_prediction_tokens` 均为 `null`。当前错误为 `invalid type: null, expected u32`（line 1 column 239）。

## What Changes

- 将 Chat usage 中上述可选明细按可空计数解码：缺失或 `null` 为未知，显式 `0` 和正整数保留其值。
- 保持可信 prompt/completion/total、cached tokens 和现有账本结构不变；`TokenUsage` 聚合的 reasoning 未知值仍沿现有边界折算为 0。
- 保持严格输入校验：负数、字符串、浮点、溢出及必需总量为 `null` 仍拒绝；不跳过坏 chunk，也不改变重试策略。

## Capabilities

### Modified Capabilities

- `model-sampling`：规定可选 Chat usage 明细对缺失与显式 `null` 的处理，并验证有效完成响应仍完整结算。

## Impact

实现范围为 sampling types 与 Chat Completions wire 解码及其测试。验证需覆盖 streaming 和 non-streaming decode、严格 nullable 边界、HTTP SSE 的 tool-call 完成顺序与结算，以及使用本机保存的原始响应离线复验（不改 session 原文件）。

## Evidence and Status

本提案记录已复现的协议解码故障；产品代码和验证尚未开始。现有 `model-sampling` 的 cache usage 与 completed-attempt settlement 契约仍适用，本 change 不改账本、恢复或重试契约。
