## ADDED Requirements

### Requirement: Skill expansion preserves selected source identity
技能展开 SHALL 同时匹配解析时选定的路径、限定名称与插件身份，不得仅凭同一路径使用其他来源条目。

#### Scenario: Native and plugin skills share a path
- **WHEN** 原生技能和插件技能路径相同，用户选定插件技能
- **THEN** 展开使用该插件条目的正文快照和插件变量。

#### Scenario: Selected source disappears
- **WHEN** 选定插件条目消失但目录仍有同路径原生技能
- **THEN** 该引用展开失败，不回退到原生条目。
