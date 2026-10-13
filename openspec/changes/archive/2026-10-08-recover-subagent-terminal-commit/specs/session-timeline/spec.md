## ADDED Requirements

### Requirement: Subagent finalization retains one accountable owner

子任务执行返回、失败、panic 或取消后，系统 SHALL 保留该 child 的收尾责任，直到所需持久事实和交付责任已确认，或明确展示仍待修复。执行 outcome、提交状态和交付状态 SHALL 分离。未确认收尾不得被普通完成缓存淘汰、作为已完成依赖使用或用于绕过源实例仍存活时的 resume 拒绝。并发推进和暂存资源 SHALL 有界。

#### Scenario: Runner exits after output publication fails
- **WHEN** runner 已返回，但 output artifact 或 child result 提交失败
- **THEN** 查询显示执行已返回且提交受阻，保留同一 child 的收尾责任；不会把它静默移入可淘汰的完成项。

#### Scenario: Panic competes with cancellation
- **WHEN** runner panic 与取消或正常收尾竞争
- **THEN** 只有一个收尾 owner 推进原 child 的事实；已确认 outcome 不被覆盖，未确认状态仍可查询。

#### Scenario: Pending finalization reaches capacity
- **WHEN** 待收尾元数据或未持久正文达到容量上限
- **THEN** 新准入受到反压，超限正文保全失败可见，既有未确认责任不被当作成功完成或静默丢弃。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::finish_child/finish_panicked_child`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`。

### Requirement: Subagent reconciliation advances only verified commit prefixes

运行期与重启恢复 SHALL 按同一 child/parent 身份链核对已提交前缀，依序补齐结算、结果、父引用和回执。重试 SHALL 保留原 immutable event identity 与 payload；ACK 不明不得解释为未写入。writer 失效后 SHALL 先满足完整 writer drain、重新取得独占 lease 并校验记录，才执行后续写入。普通观察不得隐式触发修复。

#### Scenario: Commit succeeds but acknowledgment is lost
- **WHEN** 任一收尾事实可能已写入但调用方未取得 ACK
- **THEN** 系统核对或 exact 重放该事实并完成所需同步屏障，不追加重复事实、不改变原 outcome，也不重发 provider 或工具。

#### Scenario: Space becomes available after writer failure
- **WHEN** StorageFull 使 writer 失效，随后磁盘空间恢复
- **THEN** 只有旧 actor/persistence owner 结束且 lease 释放后，新 owner 才能校验原记录并继续收尾；旧 handle 不被重新视为有效 writer。

#### Scenario: Actor exited while persistence still owns storage
- **WHEN** actor 已退出但 persistence task 或已派发 I/O 尚未完成
- **THEN** 替代 writer 仍被拒绝，等待者可得到待确认状态，原写入所有权不会因超时被清除。

#### Scenario: Parent terminal already exists
- **WHEN** child result 与 parent Ended 已通过严格链接校验，但回执或展示投影缺失
- **THEN** 恢复只补缺失部分，保留原终态及一次性的用量累计。

#### Scenario: Conflicting evidence is found
- **WHEN** 同 identity 的 payload、序号或父子链接冲突
- **THEN** 停止自动收尾并保留证据，不覆盖记录或猜测另一份结果。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::reconcile_orphaned_subagents_with_backend/ensure_recovered_child_result_with_opened`、`crates/codegen/shell/src/session/storage/jsonl/mod.rs`、`crates/codegen/chat-state/src/actor/mod.rs`。

### Requirement: Recovered child outcomes preserve evidence and availability

已经验证的 child terminal outcome SHALL 保持不变，正文可用性与交付失败分别报告。只有证明原执行实例已结束、合法 writer 已取得所有权且无有效终态时，恢复才可在结算未闭合 scope 后记录 Recovery 来源的 interrupted 结果。未知用量 SHALL 保持 incomplete，未知时间不得以整个离线间隔冒充执行耗时。

#### Scenario: Completed child loses its output artifact
- **WHEN** child terminal 及父子身份可验证，但结果正文文件缺失或校验失败
- **THEN** 已确认 outcome 仍可只读展示，并明确正文不可用；不得改写成新的失败 outcome 或拼凑正文交付。

#### Scenario: Inspection failed while child may still run
- **WHEN** backend 不可达或超时，无法确认旧实例已退出
- **THEN** 保持收尾待确认，不生成 interrupted/cancelled 结果，也不抢占 writer。

#### Scenario: Cold recovery finds an interrupted child
- **WHEN** 新 writer 验证原实例已结束、没有有效 child terminal 且还有未闭合执行或结算
- **THEN** 先按既有结算规则恢复这些 scope，再记录明确的恢复中断来源及用量下界；不伪造用户取消或零消费。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::result_from_inspection/finish_from_durable_facts`、`crates/codegen/chat-state/src/timeline.rs::SubagentResultEvent`。

### Requirement: Finalization wait deadlines preserve durable delivery ownership

单次收尾推进与回执交接的 caller 等待 SHALL 默认总计不超过三十秒，回执每次 ACK 等待不超过五秒，调用方更早期限优先。收尾写入重试 SHALL 共享本周期绝对期限，期限到达后不再派发新的 append。等待超时 SHALL 返回提交/交付未确认而非虚构执行失败，且不取消仍需持有的写入所有权。未确认操作仍在途时不得叠加 mutation 重试；明确可重试的回执尝试每周期至多三次。迟到结果 SHALL 通过原身份的持久回执交接，每个 tool call 只提交一份 tool_result。

#### Scenario: Receipt producer remains alive without replying
- **WHEN** 回执 ACK sender 始终存活但不回复
- **THEN** caller 在期限内退出等待并报告交付未确认，原操作继续受 owner 管理，不因 sender 存活无限挂起 caller。

#### Scenario: Caller already received a pending result
- **WHEN** 等待工具已结束原 tool call，之后 canonical terminal 和回执提交成功
- **THEN** 结果经既有通知交付，不给原 call id 再提交第二份 tool_result，回执消费按原 identity 去重。

#### Scenario: Transient failures exhaust the recovery period
- **WHEN** 收尾持久化持续返回可重试错误直至本周期期限
- **THEN** 不再派发新的重试，返回待修复状态并保留责任；重连不重新起表，在途 I/O 的所有权保持到其实际结束。

#### Scenario: Parent writer also fails
- **WHEN** 父会话无法持久接纳 tool_result 或回执
- **THEN** 显示未确认交付并保留恢复事实，不把内存发送成功声称为持久接纳。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::admit_notification_receipt`、`crates/codegen/shell/src/session/actor/completion_delivery.rs`、`notification_drain.rs`。
