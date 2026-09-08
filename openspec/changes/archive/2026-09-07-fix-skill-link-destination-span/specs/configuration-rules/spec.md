## ADDED Requirements

### Requirement: Skill link resolution edits only destination text
技能内部链接解析 SHALL 只修改目的地址源码范围，保持标签、标题和非链接文本不变。

#### Scenario: 目标和标题同名
- **WHEN** 技能正文链接的目标与 title 都包含同一文件名
- **THEN** 只将目标解析为技能目录内路径，title 保持原文。

#### Scenario: 标签和代码中的同名文本
- **WHEN** 链接标签、嵌套图片或代码片段包含同名地址
- **THEN** 仅实际链接/图片的目的地址被改写，代码和标签文本保留。

#### Scenario: 特殊字符路径
- **WHEN** 目标包含 Markdown 转义括号、字符实体或空格
- **THEN** 新目标仍能被 Markdown parser 解析为正确的技能内绝对路径。
