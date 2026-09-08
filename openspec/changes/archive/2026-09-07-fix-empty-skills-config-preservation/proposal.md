## Why
save_config_at 在 SkillsConfig 为默认值时删除整个 skills 段，导致未知字段在无关设置编辑或清空最后一个已知项后丢失，与既有未知字段保留策略不一致。

## What Changes
默认技能配置只移除已知字段；未知字段存在时保留该段，真正空段仍删除。字段集合取自现有 SkillsConfig 序列化结果，不维护第二份字段名单。

## Capabilities
### Modified Capabilities
- configuration-rules: 空技能配置保留未知字段。

## Impact
Shell 保存合并的 skills 默认分支；不改变 reset 对已知字段的清空行为。
