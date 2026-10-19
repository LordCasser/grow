# local-coordination Specification Delta

## ADDED Requirements

### Requirement: Inquiry identity and replay are durable across session recovery
协调询问 SHALL 以 `(source_session_id, inquiry_id)` 作为逻辑身份；target session 由接收作用域隐含。peer identity 与 incarnation SHALL 用于连接认证、租约及审计归因，不得替代 source session 成为逻辑询问身份。target Timeline SHALL 防止相同逻辑身份的询问重复执行；source Timeline SHALL 固定首次启动时的目标和问题，并使重试、恢复和响应关联服从该绑定。

#### Scenario: UUID text aliases cannot create another identity
- **WHEN** 调用方将现有 UUIDv7 改写为大写、无连字符、URN 或带括号的形式来询问、查询或取消
- **THEN** 请求因不符合 runtime 生成的小写连字符标准形式而被拒绝，不按另一字符串键创建新询问。

#### Scenario: Source reconnects with a new peer incarnation
- **WHEN** 同一 source session 在 peer incarnation 变化后以相同 inquiry_id 重连并重试相同询问
- **THEN** target 将其识别为同一逻辑询问并复用已有 receipt/outcome，不因 incarnation 变化再次采样。

#### Scenario: Different source sessions use the same inquiry ID
- **WHEN** 两个不同 source session 对同一 target 使用相同 inquiry_id
- **THEN** target 将它们作为两个独立询问处理，各自保持独立 receipt 与 outcome。

#### Scenario: Target replays a completed inquiry
- **WHEN** target 收到已有 canonical Timeline 终态的相同 source/inquiry identity 与相同 payload
- **THEN** target 复用已记录 outcome，不再次执行模型询问。

#### Scenario: Target receives an inquiry with an open receipt
- **WHEN** target 对相同 source/inquiry identity 已有未终结 receipt
- **THEN** target 拒绝重复执行，不写第二份 receipt 或启动第二次 Sideband。

#### Scenario: Target restores an open inquiry receipt
- **WHEN** target 冷恢复时发现已有 open receipt 且尚无终态
- **THEN** target 将该 receipt 收束为 `TargetRestarted`，同身份重试复用该终态且不重新采样。

#### Scenario: Source retries the original operation
- **WHEN** source 对已有 `OutgoingStarted` 身份使用相同 target 和 question 重试
- **THEN** source 沿原询问 identity 查询或复用已有结果，不创建新操作。

#### Scenario: Source retries with a changed target or question
- **WHEN** source 对已有 `OutgoingStarted` identity 更换 target 或 question
- **THEN** source 拒绝冲突，不覆盖首次绑定或 dispatch 新询问。

#### Scenario: Source restores an inquiry without a terminal outcome
- **WHEN** source 恢复时发现询问已启动但没有完成 outcome
- **THEN** 查询返回 `SourceUnavailable`，不自动执行或重发询问；相同 target/question 的显式重试仍受 target durable 去重约束。

#### Scenario: Progress or outcome carries a mismatched inquiry ID
- **WHEN** 当前询问收到 inquiry_id 与请求身份不符的 progress 或 outcome
- **THEN** 调用方返回结构化关联失败，不把该响应归入当前询问，也不自动重发。

#### Scenario: Cancel response carries a mismatched inquiry ID
- **WHEN** 当前取消请求收到 inquiry_id 与请求身份不符的响应
- **THEN** 调用方返回结构化关联失败，不将其他询问视为已取消或接受取消。

#### Scenario: Cancellation wins before dispatch
- **WHEN** source 在询问 dispatch 前已为同一逻辑身份登记取消
- **THEN** 询问不被派发，取消结果按该身份返回。

#### Scenario: Terminal remote inquiry is cancelled
- **WHEN** source 对已终结的远端询问发送取消
- **THEN** target 返回 `accepted=false`，不改变已有 outcome。

#### Scenario: Cancellation is scoped to its source and target
- **WHEN** 调用方尝试以错误 source 或 target scope 取消询问
- **THEN** 请求拒绝且不影响其他询问的执行或状态。
