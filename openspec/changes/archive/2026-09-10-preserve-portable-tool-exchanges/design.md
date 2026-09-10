## Context

数据链为 Sampler 完整响应 → ChatState durable admission → 工具执行/结果追加 → request projection → backend wire。unsigned thinking 正确地撤下 native，但此刻 prefix 末端只包含 assistant，后续结果仍在 live suffix；分别转换导致前者删除调用，后者发出孤立结果。现有 portable projector 还主动把完整往返写成 user-role Historical tool exchange，失去执行历史的结构。

## Goals / Non-Goals

目标是保留 Timeline 中已经存在、身份及配对明确的工具事实，让 portable 与 live 的切点不改变完整往返。保持当前 same-route native 精确保留，以及压缩、模型切换、恢复、签名失败后的 native 撤销。

不恢复旧签名/加密推理，不执行历史工具，不按冒号或短句推断完成，不新增恢复循环或改变合法 end_turn。真实模型是否不再提前结束需要线上证据，mock 只证明发送内容和生命周期。

## Decisions

1. **在现有 portable projector 保留中性工具协议。** 从连续的 assistant/tool_result 往返构造清除诊断后的 AssistantItem 与对应 ToolResult，保留名字、原始合法 JSON、配对 ID 和图片。工具结果继续具有 tool-role 权限，不伪装成用户或 assistant 指令。不保留无结果的调用、孤立/歧义结果或无效调用；Timeline 与既有 IntegrityRepair 仍保存原始证据。
2. **投影范围闭合在结果之后。** NativeContinuationProjection 共享计算实际 portable 末端，吸收切点之后连续的工具结果和可丢弃的可见 reasoning，但不越过真实用户/assistant/system 消息或首个 native span。converter、ChatState 估算和来源证据共用该计算，不修改 Timeline 或 native epoch 所有权。
3. **复用三个现有 backend 编码器。** Chat 使用 tool_calls/tool_call_id；Responses 使用无 provider item id 的 function_call/call_id 与 function_call_output；Messages 使用 tool_use/tool_result。same-route native spans 保留完整返回内容；中性历史不带旧 reasoning carriers。
4. **限定 backlog 闭环。** 同时解决“压缩后的 portable 工具历史”和“无签名 Messages 回退切断工具往返”。签名分片语义无复现证据，合法 end_turn 的强制继续会更改用户停止语义；用量、预览补传、Summary 读取、Workflow、UI 等独立问题不混入此次投影修复。

工具图片的既有回归还暴露了直接相关的编码缺口：`ToolResultItem.images` 可保存图片被预算淘汰后的 `ContentPart::Text`，三个 backend 先前只处理 Image 分支。保留结构化往返后必须同时保留这些文本；本变更补齐 Text 分支，Messages 复用现有附件转换器。这里只修复现有附件事实的编码，不改图片预算或资产生命周期。

## Protocol evidence

- [阿里云 Messages](https://help.aliyun.com/zh/model-studio/anthropic-api-messages)：明确 signature_delta 当前为空字符串；工具往返分别用 assistant.tool_use 和 user.tool_result，后者 ID 对应前者。本 session 的空签名不是足以拒绝所有后续请求的异常。
- [Claude preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)：keep-tail compaction 可清除旧 thinking/redacted_thinking 并保留 text/tool_use；其他模型的追加输出也按 text/tool_use 表达。仍有效的原生工具轮次必须保留完整 native thinking，本变更不削弱该通道。
- [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling)：调用与结果通过 call_id 配对；原生 reasoning 工具轮次应回传 reasoning。本次 portable 使用既有中性构造器，Responses 不伪造 provider 输出 item id/status，不把跨模型历史冒充原生 continuation。

以上为文档及编码结构证据，不等于所有供应商线上模型的接受认证。

## Risks / Trade-offs

- 旧测试将“不存在任何工具协议”作为 portable 的要求；按新契约改为完整配对、保留事实和无 opaque carrier，保留失败/恢复场景。
- token 估算必须使用同一闭合边界，避免工具参数、结果或图片重复计算/漏算。
- 不明确或未完成的原始工具记录继续由既有完整性修复负责，不能用投影静默修改持久化事实。
