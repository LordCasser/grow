## MODIFIED Requirements

### Requirement: Suspension retries retain human wait evidence

挂起 SHALL 从当前周期的持久交互事实和匹配恢复 checkpoint 明确保留的等待保留原调用、等待种类及 attempt。物理 owner 退出或首次挂起超时 SHALL NOT 把未回答等待变成 OutcomeUnknown 或 tool_result。已回答、取消、失败或未被匹配恢复 checkpoint 保留的历史交互 SHALL NOT 被复活。

#### Scenario: Owners drain after the first deadline
- **WHEN** 人工问题、权限或计划审批 owner 在挂起中释放，checkpoint 前首次挂起因期限失败，随后以同 operation 重试
- **THEN** 重试 checkpoint 保留 AwaitUser 或 AwaitPermission 与原调用依赖，原 attempt 合法结算为 Suspended，恢复重建原等待而不伪造结果。

#### Scenario: A wait was already resolved
- **WHEN** Timeline 已记录交互的真实解决，或历史 Suspended 未被匹配恢复 checkpoint 保留
- **THEN** 本次挂起不因旧 Opened 事实恢复该等待，不覆盖真实完成结果。

#### Scenario: Question receiver closes before service shutdown
- **WHEN** 挂起已关闭执行准入，前台 owner 先释放问题接收器，而问题服务 shutdown token 尚未取消
- **THEN** 该交互按 Suspended 结算并保留原等待资格，不能因清理顺序被记录为业务取消；普通非挂起的接收器关闭仍沿原取消语义。

### Requirement: Suspension wait evidence spans admission closure
人工等待 SHALL 在当前执行周期内保留因挂起结算的 attempt，即使 Suspended 早于该节点 SuspendRequested。ExecutionReleased SHALL 隔离未被匹配 checkpoint 明确保留为 AwaitUser/AwaitPermission 的历史证据；真实交互解决和工具结果 SHALL 不被旧等待覆盖。

#### Scenario: Child wait settles before node suspension intent
- **WHEN** root 或 child 准入已关闭，问题、权限或计划审批先记为 Suspended，节点随后提交挂起意图，且首次 drain 超时后重试
- **THEN** checkpoint 与冷恢复保留原 AwaitUser/AwaitPermission，不伪造 OutcomeUnknown 或工具结果。

#### Scenario: A previous suspension was released
- **WHEN** 历史 Suspended 之后存在 ExecutionReleased，再进入新的挂起周期
- **THEN** 只延续匹配 checkpoint 明确保留的逻辑等待，其余历史 attempt 不复活；后续新 attempt 与真实解决事实优先。

#### Scenario: Re-suspend before a restored human wait rebinds
- **WHEN** 问题、权限或计划审批的 checkpoint 已恢复并放行，但物理 worker 尚未建立新 attempt，此时再次挂起
- **THEN** 新 checkpoint 保留原逻辑等待且无伪造工具结果，不将其转为 OutcomeUnknown；真实回答或工具结果不得被旧 checkpoint 覆盖。
