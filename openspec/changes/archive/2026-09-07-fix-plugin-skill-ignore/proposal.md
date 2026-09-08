## Why
SkillsConfig.ignore 声明排除指定路径下技能，但插件技能在原生路径过滤完成后追加，从未应用该配置。因此被忽略的插件目录仍出现在实际发现列表中。

## What Changes
在插件技能参与合并去重前应用既有 filter_skills，不改变原生过滤时机或路径前缀规则。

## Capabilities
### Modified Capabilities
- configuration-rules: 插件技能发现遵守 ignore 路径。

## Impact
仅 agent 技能发现编排；不修改插件启用、信任、动态发现策略或全局插件注册。
