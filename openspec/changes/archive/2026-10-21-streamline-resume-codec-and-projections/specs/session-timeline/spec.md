## ADDED Requirements

### Requirement: Timeline wire decoding rejects ambiguous typed fields

Timeline 解码 SHALL 保留现有 canonical wire、任意合法字段顺序与 JSON Value 调用入口，并拒绝未知 envelope/event 字段和重复的 typed envelope/event 字段。不得通过先构造通用 Value、以 last-wins 覆盖重复字段来接受歧义事件。版本不兼容仍 SHALL 在存储读取边界先报告准确 persisted/current schema，不以解析未来 payload 的错误掩盖版本错误。

#### Scenario: Valid fields arrive in another order
- **WHEN** 合法 Timeline 记录的 event、type 与公共字段重新排列，或调用方从 JSON Value 解码
- **THEN** 得到与 canonical 编码一致的事件，恢复结果不变。

#### Scenario: Typed field repeats or an unknown field appears
- **WHEN** 原始记录重复 typed envelope/event 字段，或带未知 envelope/event 字段
- **THEN** 解码明确拒绝，不覆盖前一个值或发布部分恢复状态。

#### Scenario: A future schema has an unknown shape
- **WHEN** 存储记录的 schema 与当前版本不兼容且 payload 缺失或是未知形状
- **THEN** 先返回 persisted/current schema mismatch，仍拒绝恢复。
