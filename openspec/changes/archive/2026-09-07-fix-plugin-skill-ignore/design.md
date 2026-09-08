## Evidence
list_skills_with_plugins 先 filter_skills(native, ignore)，之后 collect_plugin_skills 并 merge。函数说明声称 ignore 应用于所有来源，而实现遗漏了插件来源。

## Decision
插件候选收集后、merge 前调用既有 filter_skills。保留 canonical path 前缀匹配和 tilde 展开；不把现有原生过滤移动到 dedup 后，避免破坏低优先级同名候补。插件目录也可含 command md，统一经过同一候选集合。

## Verification
真实临时 native 与 plugin 目录共存，分别忽略 plugin 根目录、SKILL.md 文件，保留原生同名和未忽略插件 sibling。对照空 ignore，确认插件确实被发现，而非 registry 未启用导致假阳性。运行 agent 技能模块全部回归。
