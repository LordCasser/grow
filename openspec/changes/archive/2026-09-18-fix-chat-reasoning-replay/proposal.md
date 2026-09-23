## Why

会话切换到 opencode/deepseek-flash 后，Chat Completions thinking 请求因历史 assistant 缺少 `reasoning_content` 被 400 拒绝。已有恢复只识别 Responses 的 `reasoning_text`，主规范也明确排除了 Chat；本次扩展这一已确认的契约缺口。

## What Changes

- 将现有 reasoning 回传拒绝事实和路由投影状态推广为带 backend 的同一恢复机制，精确识别 Chat `reasoning_content` 与 Responses `reasoning_text`。
- 当前 Chat 路由明确要求后，按 assistant 边界回传已有可见 reasoning；历史没有 reasoning 时输出空字符串，保留工具往返及有效 native 内容。
- 保持一次启用、确认后重提交、共享 attempt/deadline、真实路由替换清除状态的边界。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：将 provider-required portable reasoning 恢复扩展到 Chat Completions。

## Impact

涉及 sampling-types 的错误分类与请求投影、sampler DTO、ChatState 命令及瞬态路由状态、Shell 恢复分支及回归。无新增依赖、持久化实体或模型名规则；不改变工具执行、Goal 生命周期和切换调度，不改用户会话数据。
