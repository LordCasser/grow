## ADDED Requirements

### Requirement: Session usage is a durable lifetime projection

Session Timeline SHALL 持久记录每个主模型 attempt 的已知/未知 Usage、每个子 Agent 的最终 Usage 结算，以及 session-level incomplete 事实。冷恢复 SHALL 按结算身份去重重建 lifetime aggregate、provider/model 分项和 Agent 归属；不得把已有消费重置为零或重复计费。

#### Scenario: Restore settled main attempts
- **WHEN** Timeline 含多个已持久化主模型 attempt 结算并创建新的 actor incarnation
- **THEN** 新 actor 在接受后续调用前恢复这些 attempt 的累计 token、model、cost、duration 与 incomplete 状态，每个 attempt 至多计入一次。

#### Scenario: Restore settled child usage
- **WHEN** 父 session 已持久结算一个子 Agent 的多模型 Usage 后冷恢复
- **THEN** lifetime 总计、provider/model 分项与该 `subagent_id` 的 Agent 分项均恢复；相同结算重放不重复累计，冲突结算失败关闭而非覆盖。

#### Scenario: Restore incomplete accounting
- **WHEN** session-level incomplete 事实已持久化
- **THEN** 后续 resume 仍将 lifetime Usage 表示为已知下界并隐藏不可信 cost，不因进程重启恢复为精确账本。

### Requirement: Cold resume creates a durable usage segment boundary

成功发布新的冷恢复 actor incarnation 前，session SHALL 持久提交一个 Usage resume boundary。恢复投影 SHALL 以初始运行和这些边界切分结算，aggregate SHALL 等于所有 segment 的累计结果；resident client reconnect 不创建新的 actor segment。

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
