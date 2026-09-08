## ADDED Requirements

### Requirement: Settings updates edit raw config values
设置读改写 SHALL 从原始 TOML 构造编辑值，不应用环境展开或运行时版本覆盖。未修改的环境变量引用 SHALL 保持字面形式；运行时加载仍按原规则解析。

#### Scenario: Unrelated setting update with environment reference
- **WHEN** skills.paths 包含环境变量引用，读改写仅修改 disabled
- **THEN** paths 仍保留原始引用，disabled 更新正常保存。
