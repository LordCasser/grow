## Why

记录中的 ACP prompt Clippy `never_loop` 仍存在；backlog 的 readiness 段落仍将已归档修复列为待处理。

## What Changes

- 将必定单次退出的 loop 改成返回值的 `tokio::select!`，保留 completion 优先、durable ACK 之后才发布 Working 的既有顺序。
- 校正已闭合 readiness 状态；明确延期的 ACP 容量实验提案、准备文档及实现保持基线不变。
- 记录证据门槛与当前无法执行的现场验证，避免把准备、静态风险或旧结论计为本轮修复完成。

## Impact

仅无行为变化的表达简化和文档维护，`skip_specs: true`；不改变协议、预算、公开接口、持久格式或生产策略。
