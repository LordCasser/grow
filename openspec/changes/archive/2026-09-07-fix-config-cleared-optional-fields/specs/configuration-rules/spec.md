## ADDED Requirements

### Requirement: Settings updates persist explicit field clearing
设置读改写 SHALL 删除修改前存在、修改后明确清空并不再序列化的已知字段，包括嵌套字段。未修改和未知字段 SHALL 保留。

#### Scenario: Restore optional setting defaults
- **WHEN** 取消子任务策略、屏幕模式或默认模型被设为 None
- **THEN** 对应旧配置键删除，重新读取使用默认值。

#### Scenario: Nested clearing with unknown siblings
- **WHEN** 一个已知嵌套字段被清空，同表还有未知字段
- **THEN** 仅已知字段删除，未知字段保留。
