## ADDED Requirements

### Requirement: Tree admission freeze has a durable suspension identity
树级挂起 SHALL 在首次 coordinator freeze 副作用前持久确认 root operation。冻结后的初始化超时或 ACK 未确认 SHALL 保留该身份，重试 SHALL 复用它，不允许覆盖不同 owner。

#### Scenario: Initializing child exceeds the freeze deadline
- **WHEN** coordinator 已冻结，但 child 初始化未在期限内完成
- **THEN** root 保留持久挂起身份，重试继续同 operation 并重新核对 child，不发生 conflicting suspension owner。

#### Scenario: Freeze acknowledgement is lost
- **WHEN** coordinator 接纳冻结后 ACK 丢失
- **THEN** 首轮报告未确认，重试向 coordinator 发送相同持久 operation。

### Requirement: Suspension wait evidence spans admission closure
人工等待 SHALL 在当前执行周期内保留因挂起结算的 attempt，即使 Suspended 早于该节点 SuspendRequested。ExecutionReleased SHALL 隔离前次恢复周期证据；真实交互解决和工具结果 SHALL 不被旧等待覆盖。

#### Scenario: Child wait settles before node suspension intent
- **WHEN** root 或 child 准入已关闭，问题、权限或计划审批先记为 Suspended，节点随后提交挂起意图，且首次 drain 超时后重试
- **THEN** checkpoint 与冷恢复保留原 AwaitUser/AwaitPermission，不伪造 OutcomeUnknown 或工具结果。

#### Scenario: A previous suspension was released
- **WHEN** 历史 Suspended 之后存在 ExecutionReleased，再进入新的挂起周期
- **THEN** 不复活历史 attempt，只按该执行周期的新交互事实恢复等待。

## MODIFIED Requirements

### Requirement: Suspension retries retain human wait evidence

挂起 SHALL 从最近 ExecutionReleased 之后、归属于当前 operation 的持久交互事实保留原调用、等待种类及 attempt。物理 owner 退出或首次挂起超时 SHALL NOT 把未回答等待变成 OutcomeUnknown 或 tool_result。已回答、取消、失败或前次挂起已关闭的交互 SHALL NOT 被历史等待复活。

#### Scenario: Owners drain after the first deadline
- **WHEN** 人工问题、权限或计划审批 owner 在挂起中释放，checkpoint 前首次挂起因期限失败，随后以同 operation 重试
- **THEN** 重试 checkpoint 保留 AwaitUser 或 AwaitPermission 与原调用依赖，原 attempt 合法结算为 Suspended，恢复重建原等待而不伪造结果。

#### Scenario: A wait was already resolved
- **WHEN** Timeline 已记录交互的真实解决，或 Suspended 属于最近 ExecutionReleased 之前的历史 attempt
- **THEN** 本次挂起不因旧 Opened 事实恢复该等待，不覆盖真实完成结果。

#### Scenario: Question receiver closes before service shutdown
- **WHEN** 挂起已关闭执行准入，前台 owner 先释放问题接收器，而问题服务 shutdown token 尚未取消
- **THEN** 该交互按 Suspended 结算并保留原等待资格，不能因清理顺序被记录为业务取消；普通非挂起的接收器关闭仍沿原取消语义。
