## Why

核对用户提出的 `/btw` 上下文丢失是否同样影响压缩 Sideband，区分默认原文路径与超长降级路径，不由共享 Sideband 名称推断相同输入语义。

## What Changes

仅记录源码与现有测试证据，并将压缩简化输入移除工具结果的问题单独登记 backlog；不修改运行时或行为契约，因此 `skip_specs: true`，不新增 delta。

## Impact

只涉及本 change 的审计记录和 `openspec/backlog.md`。压缩策略改动需要单独的行为 change。
