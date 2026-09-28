## MODIFIED Requirements

### Requirement: Session usage is a durable lifetime projection

Session Timeline SHALL 持久记录每个主模型 attempt 的已知/未知 Usage、每个子 Agent 的最终 Usage 结算，以及 session-level incomplete 事实。缓存读写可用性、已知桶累计和覆盖分母 SHALL 作为原结算事实的一部分保留；总量已知但分类未知 SHALL NOT 被改为整个总消费不完整。冷恢复 SHALL 按结算身份去重重建 lifetime aggregate、provider/model 分项和 Agent 归属；不得把已有消费重置为零或重复计费。已知用量事件 SHALL 在 actor 发布及恢复事件持久化前完成载荷校验；格式损坏或同身份冲突 SHALL 使恢复失败并保留原始记录。

#### Scenario: Restore settled main attempts
- **WHEN** Timeline 含多个已持久化主模型 attempt 结算并创建新的 actor incarnation
- **THEN** 新 actor 在接受后续调用前恢复这些 attempt 的累计 token、model、cost、duration、incomplete 状态及缓存桶可用性和覆盖分母，每个 attempt 至多计入一次。

#### Scenario: Restore settled child usage
- **WHEN** 父 session 已持久结算一个子 Agent 的多模型 Usage 后冷恢复
- **THEN** lifetime 总计、provider/model 分项与该 `subagent_id` 的 Agent 分项均恢复；相同结算重放不重复累计，冲突结算失败关闭而非覆盖。

#### Scenario: Restore incomplete accounting
- **WHEN** session-level incomplete 事实已持久化
- **THEN** 后续 resume 仍将 lifetime Usage 表示为已知下界并隐藏不可信 cost，不因进程重启恢复为精确账本。

#### Scenario: Exact duplicates and conflicting attempt payloads
- **WHEN** 同一主模型 attempt 或子 Agent 身份有多份用量结算
- **THEN** 完全相同载荷只计一次；任一载荷冲突使恢复失败，不选择第一份、不覆盖，并采用与实时结算相同的冲突规则。

#### Scenario: Malformed known usage facts fail before publication
- **WHEN** 已知 attempt/child 结算无法按既有 typed schema 解码，或 incomplete/resume 标记包含不属于其格式的 data
- **THEN** 恢复返回带事件位置的错误，不发布可用 actor、不写入恢复事件，原持久化记录保持不变。

#### Scenario: Unrelated diagnostic observations remain extensible
- **WHEN** Timeline 包含不属于已知用量 scope/name 的合法 Observation
- **THEN** 用量恢复忽略该诊断事实，不将其当成损坏的结算；其他 Timeline 校验仍执行。

#### Scenario: Child cache details are partially available
- **WHEN** 子 Agent 的最终账单包含总消费已知但部分 attempt 缓存明细缺失的多模型汇总
- **THEN** 父 session 保留已知桶、未知状态与覆盖分母，各聚合维度只折叠一次，不能将分类未知归零或把精确总消费降为未知。

#### Scenario: Conflicting cache availability
- **WHEN** 相同结算身份的重复载荷在缓存可用性或覆盖计数上冲突
- **THEN** 实时和恢复均沿既有冲突规则失败，不按重复成功忽略，也不覆盖先前事实。

### Requirement: Cold resume creates a durable usage segment boundary

成功发布新的冷恢复 actor incarnation 前，session SHALL 持久提交一个 Usage resume boundary。恢复投影 SHALL 以初始运行和这些边界切分结算，aggregate SHALL 等于所有 segment 的累计结果，包括各缓存桶已知累计、未知标记和覆盖分母；命中率 SHALL 由累计计数重新计算而非平均 segment 百分比；resident client reconnect 不创建新的 actor segment。

#### Scenario: First cold resume
- **WHEN** 已有 session 在新 actor incarnation 中成功恢复
- **THEN** 后续结算进入 Resume #1 segment，此前结算保留在 Initial run，lifetime aggregate 同时包含两段。

#### Scenario: Repeated cold resumes
- **WHEN** session 多次关闭并冷恢复
- **THEN** 每个成功发布的 incarnation 形成有序的新 segment，后续结算只进入当前段，历史段保持不可变。

#### Scenario: Resident reconnect
- **WHEN** 客户端重新连接仍存活的同一 actor incarnation
- **THEN** 不创建新的 Usage segment，既有当前段继续累计。

证据入口：`crates/codegen/chat-state/src/usage.rs`、`actor/mutations.rs`、`actor/state.rs` 与 `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs`。

### Requirement: Auxiliary provider attempts enter the owning session usage projection

Session Timeline SHALL durably record known or unknown usage for each admitted main and Sideband provider attempt, each child's final usage bill and session-level incomplete facts. Sideband attempt identity SHALL be `(sideband_id, attempt_no)` in its owning session; each attempt SHALL be counted at most once, including failed and retried requests. A cold recovery with an admitted Sideband attempt lacking terminal usage SHALL retain an incomplete lower-bound ledger. Recovery SHALL reject malformed or conflicting known billing facts before publishing an actor. A child Sideband SHALL be charged to its child ledger and reach the parent only through the child's final bill.

#### Scenario: Failed attempt followed by a successful retry

- **WHEN** one Sideband issues two provider requests under successive attempt numbers, the first fails with known usage and the second succeeds
- **THEN** the owning session records both usages once under the selected route model, while the Sideband Result is not billed again

#### Scenario: Cancellation or crash after admission

- **WHEN** a Sideband provider request has been admitted but usage cannot be confirmed before owner cancellation or cold recovery
- **THEN** the owning session retains an incomplete usage lower bound rather than an exact zero, without fabricating token counts

#### Scenario: Duplicate and conflicting Sideband bills

- **WHEN** a Sideband attempt settlement is repeated with an identical payload or with a different payload
- **THEN** the identical payload is a no-op and the conflicting payload fails closed in live and restored projections

#### Scenario: Child Sideband final bill

- **WHEN** a child consumes tokens in its main loop and a Sideband, then settles its final bill to its parent
- **THEN** the child ledger includes both attempts and the parent includes their aggregate once under that child identity

#### Scenario: Known auxiliary totals with missing cache fields
- **WHEN** Sideband 的 full input/output 已知但缓存读写字段部分缺失
- **THEN** 原 Sideband attempt 结算保留缓存可用性与覆盖数据，总消费保持精确；经过 child final bill、冷恢复和报表分段后仍保持同义。
