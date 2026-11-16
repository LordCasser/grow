## Why

用户要求完成本轮文档和任务收尾后，递增最小版本并正式发布。当前基线是 v2.6.14，本次发布 v2.6.15。

## What Changes

- 纳入已验收并归档的恢复展示修复，以及单独验收的 gap 锚点收尾。
- 同步 workspace/lock 中的本地包版本和 changelog，不更新外部依赖。
- 将本次四项恢复 PTY 接入既有 core regression；整理已确认的 Rustfmt 基线差异。
- 核对并关闭已实际发布的旧 v2.6.5 准备任务。
- 提交、annotated tag、推送并执行官方十平台发布，核对同 SHA 门禁、资产及 checksum。

## Scope

本 change 是发布与工具维护，skip_specs：不引入新产品契约。恢复和 gap 行为分别以其功能 change 为准；明确延期、无本轮失败证据的 backlog 不纳入此发布收尾。
