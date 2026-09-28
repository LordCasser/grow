## Why

将已提交并完成规范归档的变更整理为 Grow 2.2.2 发布版本，确保 workspace 版本、公开 changelog 与发布 tag 一致。

## What Changes

- 将 workspace 版本更新为 2.2.2。
- 汇总本版本的已归档功能与修复到发行说明。

## Capabilities

无产品契约变化。`.openspec.yaml` 设置 `skip_specs: true`：本 change 只准备版本元数据和发行说明，不改变运行时行为，不创建 delta。

## Impact

仅涉及 `Cargo.toml`、Cargo 锁文件、发行说明和本发布记录。构建与发布仍由现有 GitHub Actions workflow 执行。
