## ADDED Requirements

### Requirement: Suspension retries reuse canonical completion receipts

挂起 SHALL 复用相同后台任务终态身份已接纳的完成回执，包括已消费回执；仅无回执时提交 CommandsSettled 对应通知。原正文及消费状态 SHALL 保持不变，不能通过放宽异正文身份冲突或新建通知版本绕过重试。

#### Scenario: A normal completion precedes suspension
- **WHEN** 后台任务正常完成回执已接纳，挂起又收集到同一任务结算，且首次挂起失败后按同 operation 重试并恢复
- **THEN** checkpoint 与恢复成功复用原回执及正文，不重复提交或重新消费终态通知。

#### Scenario: Suspension creates the first receipt
- **WHEN** 挂起结算尚无终态回执，通知接纳后 checkpoint 前失败
- **THEN** 同 operation 重试复用该回执，恢复仅交付一次原结果。

### Requirement: Suspension retries retain human wait evidence

挂起 SHALL 从当前 operation 的持久交互边界保留原调用、等待种类及 attempt。物理 owner 退出或首次挂起超时 SHALL NOT 把未回答等待变成 OutcomeUnknown 或 tool_result。已回答、取消、失败或前次挂起已关闭的交互 SHALL NOT 被历史等待复活。

#### Scenario: Owners drain after the first deadline
- **WHEN** 人工问题、权限或计划审批 owner 在挂起中释放，checkpoint 前首次挂起因期限失败，随后以同 operation 重试
- **THEN** 重试 checkpoint 保留 AwaitUser 或 AwaitPermission 与原调用依赖，原 attempt 合法结算为 Suspended，恢复重建原等待而不伪造结果。

#### Scenario: A wait was already resolved
- **WHEN** Timeline 已记录交互的真实解决，或 Suspended 属于早于本次 operation 的历史 attempt
- **THEN** 本次挂起不因旧 Opened 事实恢复该等待，不覆盖真实完成结果。

#### Scenario: Question receiver closes before service shutdown
- **WHEN** 挂起已关闭执行准入，前台 owner 先释放问题接收器，而问题服务 shutdown token 尚未取消
- **THEN** 该交互按 Suspended 结算并保留原等待资格，不能因清理顺序被记录为业务取消；普通非挂起的接收器关闭仍沿原取消语义。
