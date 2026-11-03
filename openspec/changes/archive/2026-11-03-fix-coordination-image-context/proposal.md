## Why

多模态主会话切换为纯文本模型后，前台已经提交图片文字描述，但协调问答仍直接从 Surface 投影原图，导致 `messages.content.type` 400。现有 `/btw` 已有只读复用文字分支的契约，协调问答遗漏了同一边界。

## What Changes

- 父子与 peer 协调问答复用已确认的图片文字分支，等待主转换的 durable ACK 后再冻结上下文。
- 与 `/btw` 共用冻结入口，保留已完成工具证据、无工具执行和主图片状态只读语义。
- client、模型与图片能力身份来自同一准备配置。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `local-coordination`：协调冻结上下文复用已持久确认的图片描述，并明确等待、取消及未就绪边界。

## Impact

涉及 shell actor 的 coordination、recap、sideband 及既有图片恢复测试夹具；更新开发说明。无新增依赖、持久化格式或独立图片恢复流程，不扩展协调问答的前台行动权限。
