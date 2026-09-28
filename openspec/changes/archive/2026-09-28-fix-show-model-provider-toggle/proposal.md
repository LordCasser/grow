## Why

Settings → Models 中的 “Show model provider” 可以显示当前值，却无法用 Enter、空格或点击切换。现行 client-surfaces 契约已经要求用户能在 Settings 中启用该偏好；实际弹窗的 Bool 动作映射漏掉此键。

## What Changes

- 补齐该设置在 Settings 弹窗中的 Bool 切换动作，使现有即时显示、持久化和失败回滚链路能够到达。
- 用弹窗输入路径的回归测试覆盖开启和关闭，并明确规范中的交互触发方式。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`: 明确 “Show model provider” 行经 Enter、空格或点击切换时，必须调用现有偏好更新行为。

## Impact

仅涉及 Pager Settings Bool 动作映射、该弹窗的定点测试及开发者说明。不改变模型路由、采样、配置格式或其他设置。
