## Why

Responses 兼容端点在 thinking + tools 模式下可能要求后续无状态请求回传先前的 `reasoning_text`。当前模型/backend 切换会正确撤下 native continuation，但 portable 投影同时保留结构化工具调用并删除其 reasoning；当排队的模型切换恰好在工具响应与结果后的下一 Step 之间生效时，目标端点会以 400 拒绝请求。

## What Changes

- 将明确的 `reasoning_text` 回传要求分类为 sampler 的类型化失败事实，避免 session 从展示文本重新猜测语义。
- ChatState 在当前采样路由内学习该兼容模式，并仅为 Responses portable prefix 回放现有可见 reasoning；opaque identity、签名、加密内容和 native span 仍不跨路由复活。
- 首次明确拒绝后在同一逻辑采样预算内确认状态变更并静默重建请求；重复拒绝有界终止。
- 覆盖排队切换后的工具往返、wire 形状、路由隔离和错误分类回归。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：Responses 兼容端点要求 portable reasoning 时的窄化回放与自动恢复。

## Impact

影响 sampling-types 的 portable Responses 投影、sampler 错误 DTO、ChatState 的当前路由投影状态，以及 SessionActor 的有界恢复分支。不改变 Timeline 持久化格式、工具执行语义、模型选择队列或其他 backend 的默认请求形状。
