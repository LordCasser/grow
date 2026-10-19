## Context

Chat usage 的可信总量与可选细分明细目前共用非空 `u32` 解码形态。Serde 默认值能处理字段缺失，却不能接受字段显式为 `null`。本机 SSE 尾帧提供了该实际输入的证据；finish reason 已在前序事件到达，尾帧 parse error 随后将请求终止。

## Decision

只把 Chat wire 类型中可能为 `null` 的可选明细表达为 `Option<u32>`：`PromptTokensDetails.audio_tokens`，以及 `CompletionTokensDetails.reasoning_tokens`、`audio_tokens`、`accepted_prediction_tokens`、`rejected_prediction_tokens`。字段缺失和显式 `null` 均解为 `None`，显式零保留为 `Some(0)`，正整数保留原值。核对所有构造点及向内部 `TokenUsage` 的映射；已有的聚合规则继续将未知 reasoning 计数映射为 0，不扩展账本模型。

必需总量保持必需且非空，其他计数继续由整数类型约束；不增加宽松解析、跳过坏事件或重试分支。Chat streaming 与 non-streaming 使用的 wire 结构需保持一致。

## Verification

为每种解码入口覆盖缺失、`null`、0、正整数及非法负数、字符串、浮点和越界值；同时确认必需总量 `null` 仍失败。用完整 HTTP SSE 验证 tool-call finish 后带 nullable details 的 usage 尾帧可以完成并将可信总量结算完整。最后从 session Timeline/artifact 的只读副本读取原始尾帧，离线复现成功，不修改原 session 文件。
