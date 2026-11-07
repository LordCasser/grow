# Delta: input-admission

## ADDED Requirements

### Requirement: Stable retry of an immutable input request returns the canonical admission state

同 session/prompt 稳定身份的不可变请求重试 SHALL 先以 canonical Timeline fold
（稳定 input identity 查询）命中并只读该输入的 payload_ref 核对完整不可变
payload 与 requested_route（排除服务端冻结 skill_invocations，不剥用户正文、
图片或 route）。命中后重试 SHALL 作为观察返回原 input identity 与当前
canonical 状态，不重复 Submitted、接纳 Hook、入队或执行。服务端按 FIFO
idle/running 边界派生 Prompt 与 Followup 的差异 SHALL NOT 构成重试冲突；
显式 Steer、requested_route、正文、图片或 metadata 变化 SHALL 返回身份冲突
并保留原输入。接纳查询 SHALL NOT 为核对一个 prompt 遍历读取全部历史 blob。

#### Scenario: 已接纳请求的运行中重试

- **WHEN** 一个 FIFO 输入已按 Prompt 接纳，当前有 regular turn 运行，客户端以
  同 promptId 与相同不可变请求重试
- **THEN** 返回原 input identity 与 canonical 状态观察，intent 派生差异不报
  冲突，不重复 Hook、入队或执行。

#### Scenario: ACK 丢失后的冷重试

- **WHEN** 用户输入已持久接纳/执行，客户端丢失 ACK 后以原请求重试
- **THEN** 返回原 input identity 与当前状态（submitted/allowed/blocked/
  consumed/handled/dismissed 之一），接纳 Hook 与输入执行各只发生一次。

#### Scenario: payload 或显式 route 变化

- **WHEN** 同 session/prompt 重试携带不同正文、图片、metadata 或显式
  requested_route/intent
- **THEN** 返回身份冲突，原输入保留，新内容不触发 Hook 或入队。

### Requirement: Duplicate retry responses are observations, not turn completion

session/prompt 重复输入的应答 SHALL 使用内部 replay 完成种类携带原 input
identity 与 canonical 状态，ACP 层 SHALL 在正常 completion、usage、roster 投影
与 TurnCompleted 广播之前短路：StopReason::EndTurn 仅结束本次观察 RPC；meta
只含 sessionId、promptId 与 `grow/inputAdmission`（inputId 和按现有
input/admission_status 形状的 status）。重复分支 SHALL NOT 附 totalTokens、
usage、structured output，SHALL NOT 发布新的 Working/Idle roster 投影或伪造
RemovedFromQueue/TurnCompleted。重复分支 SHALL 先返回观察应答再回原持久确认；
未取得本次实际提交确认的接纳错误 SHALL NOT 发布假的 roster Idle。新输入的
原 Hook、错误与完成政策保持不变。直接 Steer 扩展应答形状保持，只同步消费
canonical 状态。

#### Scenario: Running 状态下重试已接纳输入

- **WHEN** 输入已接纳（例如 idle 时按 Prompt 接纳）且当前有 regular turn
  运行，客户端以同 promptId 与相同不可变请求重试 session/prompt
- **THEN** RPC 以 EndTurn 观察应答返回原 inputId 与当前 canonical 状态，
  不发布 Working/Idle、不附 usage/totalTokens、不发新的 TurnCompleted 或
  RemovedFromQueue。

#### Scenario: 接纳错误未获提交确认

- **WHEN** 本次提交未取得 durable 用户消息确认即失败
- **THEN** 不发布假的 roster Idle；执行开始后的错误仍沿原完成/error 路径。
