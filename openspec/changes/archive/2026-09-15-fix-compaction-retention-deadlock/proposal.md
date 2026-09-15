## Why

会话 `01a0a319-0e5c-7252-86c4-0efcee4671da` 的自动和手动压缩均在选区阶段失败：最近带 prompt index 的 NotificationDrain 后不足保留预算，整回合选区回退到更早的长片段，但细分仍从最后一个 prompt 开始，遗漏早期可压缩响应组。另有已确认的失败通知被 RPC 错误字符串化丢失终态标记，Pager 误报结果未知。

## What Changes

- 在为尾部预算选中的 prompt 片段内细分完整响应组，保留预算、原 prompt 和工具配对。
- 手动压缩扩展接口透传 actor 的 ACP 错误，保留已发布终态标记。
- 增加选区、RPC 和客户端终态回归，并以只读 Timeline 重放复核用户会话。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `context-compaction`：补充跨 prompt 尾部的细分选区和手动压缩终态透传要求。

## Impact

影响 chat-state 选区和 shell 扩展接口；Pager 只补测试。既有主规范要求局部替换及保留工具配对，但未约束此选区空洞；在本 change 增加场景。`inventory-all-crate-features` 为未归档审计，不能代替主规范。无新增依赖、存储格式或配置项，不改用户会话、Goal 状态、模型预算或自动重试策略。
