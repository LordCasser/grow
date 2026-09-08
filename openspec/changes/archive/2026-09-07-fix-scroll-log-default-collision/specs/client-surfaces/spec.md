## ADDED Requirements

### Requirement: Default scroll recordings use independent paths
默认滚动日志路径 SHALL 包含时间戳之外的独立标识，避免同秒创建的记录器复用默认文件。

#### Scenario: Same timestamp recorders
- **WHEN** 同一时间创建两个默认路径的记录器
- **THEN** 路径不同，两个记录器各自保留写入内容。

#### Scenario: Enabled recorder remains idle
- **WHEN** 生成默认路径并启用记录器但尚无记录
- **THEN** 不创建文件，保持首条记录触发创建的行为。
