## Why

图片首次发送必经文本草稿 checkpoint，但 capture_agent 明确拒绝含图快照，sync_subjects 丢弃该捕获错误后 persist_input_checkpoint 返回 input intent was not captured。此前修复仅释放失败占位，仍要求图片重发，未解决这两个边界的冲突。

## What Changes

- 显式图片请求首次发送沿用现有 RAM-only 身份保留与 ACP 发送路径，不要求文本草稿持久化图片。
- 保留普通文本 checkpoint、真实 I/O 失败处理、图片加载校验和 Shell durable admission。
- 普通图片及图片 steer 均验证真实 event-loop 到 ACP 传输边界。

## Capabilities

### Modified Capabilities
- `client-surfaces`：显式图片提交与本地文本草稿 checkpoint 分离。

## Impact

Pager effect 派发和定向测试、开发说明。不扩大草稿预算、不新增附件缓存、不修改服务端接纳或模型能力协议。
