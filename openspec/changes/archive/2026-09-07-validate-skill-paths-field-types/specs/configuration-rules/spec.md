## ADDED Requirements

### Requirement: Skill paths reject invalid field types
技能 paths 限制 SHALL 接受字符串或纯字符串列表，错误类型及混合列表 SHALL 返回解析错误，不得静默丢弃并产生无条件技能。

#### Scenario: Invalid paths type
- **WHEN** paths 为数字、布尔、映射或含非字符串的列表
- **THEN** 发现跳过技能，不将该字段视为缺省或部分有效。

#### Scenario: Valid optional paths
- **WHEN** paths 为合法字符串/字符串列表，或 null、空值、匹配全部
- **THEN** 保留既有分隔、归一化及无条件语义。
