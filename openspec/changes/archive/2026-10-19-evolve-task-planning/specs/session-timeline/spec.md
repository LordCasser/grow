## ADDED Requirements

### Requirement: Planning changes durably own their delivery obligations

局部计划 SHALL 由 SessionActor 经 Timeline 提交，TODO Resources、模型和 UI SHALL 只读取其投影。产生重规划的来源提交 SHALL 同时包含可重建的目标交付责任；跨 session 投递经既有 inbox 与 reconciliation 推进，不要求跨 session 原子事务。ACK 不明 SHALL 核对原 identity，不重复来源修改或副作用。

#### Scenario: ST-01 Source commits before delivery fails
- **WHEN** 父项变化已持久提交但投递前进程退出，或 child inbox 暂不可用
- **THEN** 合法 writer 恢复后可核对原责任并补投递；不会丢失变更或重新提交同一父项修改。

#### Scenario: ST-02 Source write fails or acknowledgment is unknown
- **WHEN** 来源写入失败或 ACK 不明
- **THEN** 不先发布成功投影或执行依赖该变更的通知；明确失败可重试，未知状态先核对原事实，不能另造 identity。

#### Scenario: ST-03 Resources or UI contains stale data
- **WHEN** Resources、UI 缓存与已验证 Timeline 计划不同
- **THEN** 从 Timeline 重建投影，缓存不能覆盖计划或重新产生变更事件。

证据与实施入口：`crates/codegen/chat-state/src/timeline.rs`、`crates/codegen/tools/src/registry/types.rs`、`crates/codegen/tools/src/persistence.rs`、`crates/codegen/shell/src/session/actor`。

### Requirement: Replanning receipt consumption and handling remain distinct

重规划 SHALL 复用 agent 消息的 durable Received/Consumed 与准确 context 提交；Handled SHALL 是 child 对指定来源集合的独立持久事实，与该次局部计划修改或 no-change 决定一起提交。恢复 SHALL NOT 把已消费消息重新消费/注入，而应保留尚未处理的工作责任；只有合法同身份冻结恢复可以继续原检查，终态执行则明确记录未应用。处理确认与父级交接 SHALL 可按原身份核对。

#### Scenario: ST-04 Consumed update has no handling acknowledgment
- **WHEN** child 已消费更新，但在提交局部计划/处理确认前完成了可验证的同身份冻结
- **THEN** 已消费消息仍只出现一次，未处理责任保留；合法树恢复通过当前工作投影继续检查，不伪造 Handled。

#### Scenario: ST-05 Handling commits but parent misses acknowledgment
- **WHEN** child 已持久确认处理，父级未收到 ACK
- **THEN** 核对原确认收敛义务，不要求 child 重改 TODO；迟到旧确认不能清除新变化。

#### Scenario: Child and parent plans advance independently
- **WHEN** child 的 Handled 回执引用其实际局部计划版本，该版本大于父计划的当前版本
- **THEN** 父级按原委派和来源身份接纳该回执，不比较两个 owner 的版本计数；child 自己仍拒绝引用尚不存在的本地版本，较新的父级变更继续待处理。

#### Scenario: ST-14 Received batch is followed by a newer update
- **WHEN** A 已 durable Received 但尚未 Consumed，B 在消费边界之前或之后到达
- **THEN** A 的正文和身份不变；边界前可按顺序消费 A/B 并一次确认覆盖二者，边界后只能先确认 A、保留 B 待处理，不能用发送方失效标记跳过接收方事实。

#### Scenario: ST-15 Unhandled update meets a verified terminal child
- **WHEN** 已消费未处理的 child 正常完成/取消，或普通崩溃恢复验证旧实例结束并记录 interrupted
- **THEN** 原消息不重复消费，变更记录为未应用并交父级决定，不复活 child；旧实例状态尚不明时保持未确认，不能提前制造终态来关闭责任。

证据与实施入口：`crates/codegen/shell/src/session/actor/notification_drain.rs`、`crates/codegen/chat-state/src/timeline.rs` 的通知消费事实。既有 “Agent opinion consumption preserves source and exact context” 继续适用。

### Requirement: Delegated task results reach their direct parent once

新接纳的普通 task 委派 SHALL 为直接父 session 保存可恢复结果交付责任，独立于 root lifecycle owner 与 Task/Goal owner。嵌套委派 SHALL NOT 仅因 root 重挂接或展示抑制而跳过直接父级完成回执。等待工具与通知 SHALL 使用同一 canonical result/receipt，最多一次逻辑模型消费，不重复 tool_result。child 最终结果 SHALL 保留本地交付的有界验收摘要/引用与未闭合必需项，不复制整棵后代结果树。Workflow journal 自有交付和真正无模型消费者的内部执行保持原协议；历史无回执记录不凭空生成新工作。

#### Scenario: ST-06 Nested result goes to the delegating child
- **WHEN** grandchild 完成，其 lifecycle spawn 存于 root，直接委派者为 child
- **THEN** child 获得原结果回执并可验收，root 保留生命周期观察但不收到第二份需消费的模型交付。

#### Scenario: ST-07 Wait completion races with steering or notification
- **WHEN** task_output 等待被用户/父指导打断，child 随后完成且通知并发到达
- **THEN** 原 tool call 至多一份 tool_result；结果按同一回执消费一次，迟到完成不丢失且不重复唤醒。

#### Scenario: ST-08 Result finalization or delivery is unavailable
- **WHEN** child 已返回，但结果 artifact、严格链接或直接父级 durable 接纳尚未确认
- **THEN** 分别显示提交/交付未确认或正文不可用，不把执行返回当作已交付，也不改写已验证终态。

#### Scenario: ST-16 Parent result summarizes its local deliverable decisions
- **WHEN** child 验收自己委派的 grandchild 并产生自身最终结果，或提前结束仍留有必需子交付
- **THEN** child 的 canonical result 保留可验证局部验收摘要/引用及未闭合项，供直接父级审计；不为 root 再生成一份 grandchild 模型交付，也不赋予 child Goal 修改权。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`、`crates/codegen/shell/src/agent/subagent/{mod.rs,handle_request.rs}`、`crates/codegen/shell/src/session/actor/completion_delivery.rs`。

### Requirement: Planning restoration preserves authority and unresolved work

计划、关联、等待、待处理重规划与未验收结果 SHALL 从持久事实恢复。与 `suspend-session-tree` 联合时 SHALL 在开放执行前恢复原树身份与依赖，遵循原预算、权限及业务暂停状态；Interrupted/OutcomeUnknown SHALL 进入检查而非工具重放。无冻结恢复权威的冷崩溃和历史 fork/rewind/export/replay SHALL NOT 因计划未完成而自动恢复执行。

#### Scenario: ST-09 Frozen tree contains both updates and interrupted commands
- **WHEN** 合法树级冻结保存了 child 等待、未处理父项变化和中断/未知命令，随后用户 resume
- **THEN** 先恢复同身份树及原责任，再在允许准入的节点检查最新要求和命令现场；不新建替代 child 或自动重复副作用。

#### Scenario: ST-10 Suspended child cannot currently receive a message
- **WHEN** 父项已提交变化而目标 child 挂起/正在恢复，原 inbox 不接纳新消息
- **THEN** 来源责任保留，合法恢复后投递；不为送达消息绕过暂停、用户/权限等待或预算 gate。

#### Scenario: ST-11 Historical state does not resume old work
- **WHEN** 普通崩溃没有有效冻结 checkpoint，或用户 fork、rewind、resume_from、导出及播放历史
- **THEN** 按原恢复/历史规则处理；旧 TODO、等待和 transcript 不能授权自动重启 child 或命令，新委派需新的当前 owner 校验。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs` 的 orphan reconciliation、`crates/codegen/shell/src/session/storage`、`crates/codegen/chat-state/src/timeline.rs`；冻结协议依赖 `openspec/changes/suspend-session-tree/`，本条不替代它。

### Requirement: Planning obligations participate in bounded retention

未闭合通知、处理确认、有效等待和必需结果验收 SHALL 保留所需身份与证据，复用现有回收规则；已处理、被合并覆盖并确认、明确取消或终态无法应用的责任 SHALL 可持久收敛。并发交接与内存排队 SHALL 有界，容量或存储失败 SHALL 可见且不得丢失已提交责任。

#### Scenario: ST-12 Cleanup overlaps pending planning work
- **WHEN** TTL 到期但变更交付、处理确认或结果验收仍需某 artifact/session
- **THEN** 必要资源保留；责任收敛后可按原清理条件回收，不靠永久保留全部历史解决交付。

#### Scenario: ST-13 Delivery capacity is exhausted
- **WHEN** 待交付内存或存储达到边界
- **THEN** 保留已提交来源责任并有界反压/重试，新接纳失败明确返回；不无限排模型轮次或将未写入变更声称成功。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs` 的清理、原 inbox 与 subagent reconciliation。
