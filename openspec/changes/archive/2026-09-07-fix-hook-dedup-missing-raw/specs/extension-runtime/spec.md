## ADDED Requirements

### Requirement: Hook deduplication tolerates missing display source
Hook 去重 SHALL 在 command_raw/url_raw 缺失时使用对应实际 command/url，不能把不同实际内容折叠为空字符串键。相同内容的 first-wins 行为保持。

#### Scenario: 不同实际命令或 URL 无 raw
- **WHEN** 多个 Hook 的 raw 字段缺失但实际 command 或 URL 不同
- **THEN** 每个不同 Hook 都保留；实际值相同时仅保留首个。
