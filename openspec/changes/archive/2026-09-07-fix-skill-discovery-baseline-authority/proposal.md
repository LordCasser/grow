## Why
动态发现直接解析文件，不包含 baseline 上应用的 disabled 等配置。add_discovered 只去重动态集合，允许后续原始解析副本遮蔽同路径的已配置 baseline。

## What Changes
已在可见 baseline 的规范路径不重复动态注册；已知条件技能沿用当前被保留的元数据和门控，不允许原始解析副本绕过。

## Capabilities
### Modified Capabilities
- configuration-rules: 动态发现尊重已加载的同路径 baseline。

## Impact
SkillManager 和 ConditionalSkills 的准入检查；不增加全局配置状态，不推断 baseline 未包含的路径被禁止。
