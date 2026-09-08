## ADDED Requirements

### Requirement: Skill envelope attributes escape special characters
技能正文块的 name/args、引用索引的 name/path 及预加载消息的 name/description/path SHALL 转义 XML 属性特殊字符，正文内容 SHALL 保持原样。

#### Scenario: Quoted skill arguments and paths
- **WHEN** 名称、参数或路径包含双引号、尖括号或 &
- **THEN** 对应属性输出实体转义，字符不得形成额外属性或标签。

#### Scenario: Body and duplicate references
- **WHEN** 正文包含 Markdown/标记且引用重复
- **THEN** 正文保持原样，引用仍按原始名称与路径去重。
