## Why

session `01a08906-7afd-70e2-af4e-5ff9ea4db84c` 的 Messages 请求在 unsigned thinking 回退后只留下 tool_result；原始响应在行动预告后合法结束。已确认的缺口来自 portable prefix 在 assistant/tool_result 之间切开，另一个相关 backlog 是完整工具往返在压缩、恢复和切换模型后全部被文本化。

## What Changes

- portable 投影保留完整且无歧义的本地工具调用及结果，复用三个 backend 的已有结构化编码；清除旧 reasoning、签名、provider item identity 和模型诊断。
- prefix 若切入工具往返，投影时将紧随的结果纳入同一范围，直到下一个消息或 native span；请求构造与 token 估算使用同一边界。
- 真实 Sampler/SessionActor 验证 unsigned thinking 工具执行到下一请求；补齐切换、恢复、局部压缩和失配边界测试。
- 关闭上述两项相关 backlog；其他领域的长期事项、合法 end_turn 自动重试、签名拼接猜测不属于本次实现。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：中性工具历史的结构化投影及跨 prefix 配对边界。
- `context-compaction`：保留 tail 中完整工具往返的模型可见结构。

## Impact

sampling-types 的 portable projector、request segmentation；ChatState 的投影估算及证据；对应 ChatState/SessionActor 测试和开发者说明。无持久化格式、配置、依赖、权限、工具执行或完成分类器变化。
