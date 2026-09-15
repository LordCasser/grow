## ADDED Requirements

### Requirement: Parked foreground waits preserve live control feedback
Pager 在前台等待工具或子 Agent 而采用 parked 显示时，SHALL 继续显示已接收的实时控制反馈，优先于后台任务静态提示；该显示 SHALL NOT 将真实前台改为 Idle、提前应用模型或新增终态事件。终态清除反馈后 SHALL 恢复原等待提示及适用的任务点击区。

#### Scenario: Model selection during a subagent wait
- **WHEN** 前台等待子 Agent，已有待处理模型切换状态
- **THEN** 状态行显示该模型转换信息，切换仍由 Shell 在 Step 边界提交。

#### Scenario: Tool wait with or without background work
- **WHEN** 前台等待工具输出，并收到控制 Pending 或 Applying 反馈
- **THEN** 无论后台任务数量是否为零，都显示反馈，并在窄终端按可用宽度裁剪。

#### Scenario: Control feedback settles
- **WHEN** 权威终态已清除实时反馈而前台仍 parked
- **THEN** 状态行恢复后台任务或 waiting 提示及原有排队、steer 语义，不重复追加完成行。
