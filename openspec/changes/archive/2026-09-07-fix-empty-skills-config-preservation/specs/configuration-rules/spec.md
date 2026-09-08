## ADDED Requirements

### Requirement: Empty known skill settings retain unknown fields
技能配置的已知字段全部清空时，保存 SHALL 仅移除已知字段并保留未知字段；仅在没有任何字段剩余时删除 skills 段。

#### Scenario: Unknown-only skill section during unrelated update
- **WHEN** skills 仅包含未知字段，用户修改其他设置
- **THEN** 未知字段保留。

#### Scenario: Last known skill field cleared
- **WHEN** 最后一个已知技能字段被清空
- **THEN** 未知子表保留；没有未知字段时 skills 段删除。
