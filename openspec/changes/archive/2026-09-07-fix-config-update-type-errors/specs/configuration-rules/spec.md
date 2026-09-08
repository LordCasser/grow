## ADDED Requirements

### Requirement: Settings updates reject invalid writable sections
设置读改写 SHALL 严格解析其写回的配置段；存在但类型错误的段 SHALL 导致错误并保留原文件，不执行修改闭包。缺失段可以使用默认值，未知字段继续按现有合并保留。

#### Scenario: Valid TOML with invalid skill field type
- **WHEN** skills.paths 为整数，用户修改其他设置
- **THEN** 返回 skills 段类型错误，原文件不变。

#### Scenario: Unknown field in otherwise valid section
- **WHEN** 合法配置段包含未知字段并修改一个已知字段
- **THEN** 已知字段更新且未知字段保留。
