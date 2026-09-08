## Why
SkillManager::update_startup_baseline 只比较路径集合。相同路径 enabled、description、调用控制等元数据变化会更新 manager baseline，但不产生 pending；ToolBridge 因而不重写 AvailableSkills，形成展示与运行时分歧。

## What Changes
同路径元数据变化应触发 reconciliation；完全相同的 baseline 仍不重复注入提醒。先验证旧行为，再确定结构化等价比较的最小实现。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能 baseline 元数据更新。

## Impact
SkillManager 的变更判断及相关类型等价比较；不引入会话 epoch，不混入并发重读重构。
