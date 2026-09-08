## ADDED Requirements

### Requirement: Scroll log status reflects recorder failure
滚动日志状态 SHALL 将已因 I/O 失败停用的记录器视为关闭，运行时切换 SHALL 可从此状态直接重新启用。

#### Scenario: Recorder fails
- **WHEN** 打开或写入失败使 recorder 进入 Disabled
- **THEN** 状态查询返回关闭，滚动输出不因诊断失败改变。

#### Scenario: Toggle after failure
- **WHEN** 失败后用户再次切换日志
- **THEN** 创建新的延迟打开记录器并返回新路径，无需先额外关闭一次；Pending/Open 仍正常切换为关闭。
