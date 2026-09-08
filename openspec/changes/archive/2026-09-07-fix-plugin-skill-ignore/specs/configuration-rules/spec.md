## ADDED Requirements

### Requirement: Skill ignore paths cover plugin candidates
技能发现 SHALL 对插件候选应用与原生来源相同的 ignore 路径过滤，且在合并去重前执行。忽略指定路径 SHALL 不影响其他路径下的同名技能。

#### Scenario: Ignored plugin directory
- **WHEN** ignore 指向插件技能目录
- **THEN** 该目录下技能不进入发现结果，其他路径的原生同名技能仍保留。

#### Scenario: Ignored plugin skill file
- **WHEN** ignore 仅指向某个插件 SKILL.md
- **THEN** 该文件条目排除，未忽略的插件 sibling 继续可见。
