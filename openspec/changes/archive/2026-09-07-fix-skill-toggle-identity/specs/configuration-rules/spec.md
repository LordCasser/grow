## ADDED Requirements

### Requirement: Skill toggles use exact catalog identity
技能开关和 disabled 配置 SHALL 使用目录去重身份：原生技能为 name，插件技能为 plugin:name。一次开关 SHALL 只改变指定身份条目，不影响其他同名身份。

#### Scenario: Plugin skill shares native name
- **WHEN** 禁用 plugin:name，且同时存在原生 name
- **THEN** 插件技能禁用，原生技能保持启用。

#### Scenario: Native skill shares plugin name
- **WHEN** 禁用原生 name，且同时存在插件的同名技能
- **THEN** 仅原生技能禁用，插件技能保持启用；列表开关发送所选条目的目录身份。
