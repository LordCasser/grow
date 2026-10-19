## ADDED Requirements

### Requirement: Internal image projection status is not assistant speech
图片工具调用及关联 carrier 的投影 SHALL 保留原有源信息清理、原始 Timeline 证据和完整 tool call/result 关联，但 SHALL NOT 用技术占位话语代替 assistant 正文。模型请求与派生 child 背景 SHALL 排除整个 assistant 正文精确等于 `[Image-associated assistant text projected to durable text.]` 或 `[Image-associated response carrier projected to durable text.]` 的内部话语。空的无调用 assistant 槽 SHALL NOT 形成空协议消息；相同文本作为 User、ToolResult 或正常解释的一部分 SHALL 保留。原始 provider/native 证据 SHALL NOT 被改写。

#### Scenario: Successful or lossy image projection
- **WHEN** description、OCR 或 unsupported-model shadow 处理图片工具调用及其 carrier
- **THEN** 后续请求保留有效调用/result 配对与适用的描述或移除结果，但不产生内部 assistant 占位话语，live 与 cold replay 保持一致。

#### Scenario: Saved assistant placeholder is reused
- **WHEN** 历史中一条 assistant 正文精确等于两个内部保留值之一
- **THEN** 派生请求与 child 背景排除该正文，保留调用及结果；若它属于当前 native span，则用既有 portable fallback 避免原生载体重新带入。

#### Scenario: Marker appears in real content
- **WHEN** 用户/工具提供同样文字，或 assistant 在正常解释中包含该文字
- **THEN** 内容按原有语义保留，不执行通用子串过滤或修改原始持久记录。
