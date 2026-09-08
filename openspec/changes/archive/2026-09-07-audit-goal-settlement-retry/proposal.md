## Why

持续审计需要证明 Goal 的已知与未知用量在 Control 写入失败后仍可结算，不能只验证成功路径。

## What Changes

- 仅补充真实 actor 的失败、恢复、重试测试及验证记录，不修改运行时行为。
- 使用已有 noop ChatStateHandle 模拟无法提交 Control，随后恢复原 handle。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

无。关联 behavior-goal 的用量归属与未知用量边界，仅增加测试；设置 `skip_specs: true`，不虚构行为 delta。

## Impact

仅 `shell/src/session/actor/goal_support.rs` 测试模块。已阅读迁移 change 的 behavior-goal delta；建项时主规范尚未归档。不修改迁移任务文件，不包含历史审计修复或计时回滚重构。
