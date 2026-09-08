## Why

验证 OpenSpec 迁移是否可交付，检查格式、代码证据、归档、链接与开发操作路径。发现历史索引依赖三份尚未跟踪的用户审计文件，单独提交迁移时会断链。

## What Changes

- 将三处本地审计链接改为带完整路径的历史说明，保留用户文件。
- 记录这次验证的结果、运行条件与限制。
- 不改变产品行为和规范契约，不修改 Rust 代码。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

无；这是文档修正与验证，skip_specs 为 true。

## Impact

仅 openspec/baseline.md 与本 change 的记录。并发的 audit-goal-settlement-retry 独立管理。
