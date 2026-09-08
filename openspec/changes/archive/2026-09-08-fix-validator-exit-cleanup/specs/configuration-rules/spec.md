## ADDED Requirements

### Requirement: Managed syntax validator exit cleanup
托管配置语法检查器 SHALL 在观察到直接子进程退出后清理其已持有进程组，再报告验证结果；清理 SHALL 使用有界等待。

#### Scenario: Successful validator leaves descendants
- **WHEN** 检查器以零状态退出且同组后代仍运行
- **THEN** 终止同组后代，再返回成功；清理失败则返回验证错误。

#### Scenario: Failed validator leaves descendants
- **WHEN** 检查器以非零状态退出
- **THEN** 执行相同清理，保留退出错误并附加清理失败信息。
