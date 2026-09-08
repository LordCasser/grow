## ADDED Requirements

### Requirement: Dynamic discovery respects known skill authority
后续动态发现 SHALL 不以未应用配置的同路径副本遮蔽已加载 baseline；已知条件技能 SHALL 保持当前门控，直到匹配激活或 baseline 更新。

#### Scenario: 重新发现停用技能
- **WHEN** 已加载 baseline 的技能停用，随后动态发现同一路径的原始启用副本
- **THEN** 保持停用状态，不为该副本产生新发现提示。

#### Scenario: 重新发现条件技能
- **WHEN** 已知未激活条件技能被再次解析且缺少当前门控字段
- **THEN** 仍保留当前门控，真实路径匹配后正常激活。
