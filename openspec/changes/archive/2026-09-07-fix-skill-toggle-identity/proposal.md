## Why
技能列表保留原生与多个插件的同名条目，但 Pager 切换发送裸 name，Shell 校验和 disabled 标记也仅比较裸名。点击一个条目会同时禁用其他同名技能，且无法单独管理插件技能。

## What Changes
使用现有 SkillInfo::dedup_key 作为配置及 toggle 身份：原生为 name，插件为 plugin:name。贯通 Pager、Shell 请求校验/响应与 agent discovery 标记。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能开关精确条目身份。

## Impact
裸 disabled 项仅指原生技能，不再作为插件同名通配符；不迁移旧配置、不新增身份类型。agent skills 声明名称匹配规则不变。
