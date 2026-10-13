## ADDED Requirements

### Requirement: Retained plan references and decisions are durable facts
计划候选、其当前资格及影响执行的决定 SHALL 由现有 Timeline/Control owner 持久记录并引用 immutable artifact。稳定决定重试 SHALL 核对原接纳/交接状态，不生成新的执行意图；未知 ACK 不构成未执行证据。feedback SHALL 只消费一次，不附加普通 HumanIntent 副本。

决定身份 SHALL 绑定不可变 action、候选提交和 feedback。同身份同内容 SHALL 返回原状态，同身份不同内容 SHALL 拒绝。只读快照未发现记录 SHALL NOT 单独证明未接纳；负面结论需要原操作明确拒绝或完整旧 writer 退出后合法 owner 的核对证据。

#### Scenario: Approval acknowledgment is lost
- **WHEN** 决定可能已提交但客户端未取得 ACK
- **THEN** 以原身份核对或重试，不重新生成 prompt 或第二次 handoff。

#### Scenario: Two clients decide concurrently
- **WHEN** 不同决定身份对同候选的批准和放弃或两次批准竞争
- **THEN** 仅一个合法决定改变权威状态，另一方得到明确过期/冲突结果。

#### Scenario: Retry changes feedback or decision
- **WHEN** 同一决定身份被用于不同 action、feedback 或候选
- **THEN** 在改变控制状态或消费反馈前返回身份冲突，原决定保持不变。

#### Scenario: Status read overtakes the original decision
- **WHEN** 查询快照未包含仍可能接纳的原决定，或旧 writer 的在途写入尚未退出
- **THEN** 返回未确认，不能授权新决定、新 prompt 或第二次 handoff。

#### Scenario: Process exits at the defer commit boundary
- **WHEN** 候选保留与清除审批 gate 的提交失败、ACK 丢失，或已提交但原工具终态尚未完成时进程退出
- **THEN** 失败/未确认不乐观释放用户内容；恢复核对原决定和工具记录，已确认保留只补合法收尾，不自动审批、实施或重弹已延后的 waiter。

实施入口：`crates/codegen/chat-state/src/timeline.rs`、`shell/src/session/actor/tool/mod.rs::admit_plan_handoff_notification`、既有 Control 提交；新字段/schema 待实现评审。

### Requirement: Retained plan recovery validates artifacts without starting work
读取或恢复展示候选 SHALL 校验引用与 artifact 的大小、hash、编码和可用性，不隐式执行、修复 writer 或从模型历史猜正文。恢复执行 SHALL 另行核对决定、handoff 消费和执行 owner。

#### Scenario: Read a retained plan after restart
- **WHEN** 用户仅打开已保留计划且 artifact 有效
- **THEN** 显示原版本，不生成实施 turn、writer 修复或审批 waiter；明确再次审阅/执行才可申请新审批关系。

#### Scenario: Artifact is missing or corrupt
- **WHEN** 候选引用的正文缺失、超限、hash 不符或编码无效
- **THEN** 明确不可用，不从旧消息拼接并执行替代正文。

实施入口：`shell/src/session/behavior.rs` artifact 读写和 `actor/tool/mod.rs::reconcile_restored_plan_approval`。

### Requirement: Retained plan artifacts participate in existing reclamation checks
有效保留候选及未确认决定/交接所需 artifact 和承载它们的 session 目录 SHALL 在既有回收所有权边界内受引用保护；UI 收起、进程离线或 TTL 本身不解除保护。放弃候选后仍按剩余历史/恢复引用判定，不直接删除正文。扫描后的引用变化 SHALL 在实际删除边界重新核对。

#### Scenario: Retained candidate outlives the cleanup window
- **WHEN** 自动清理扫描到仍有有效候选引用的计划 artifact
- **THEN** 保留正文；只有实际删除边界验证无保护引用时才按既有规则回收。

#### Scenario: Abandoned candidate still has recovery references
- **WHEN** 候选已放弃，但存在历史/未确认交接引用，或扫描后新候选引用已发布
- **THEN** artifact 与所属 session 不被旧 GC 候选列表删除；无法验证引用时保持资源并报告资格未确认。

实施入口：`shell/src/session/storage/jsonl/mod.rs` 和计划 artifact 引用；须与当前 working tree 恢复资源保全契约对齐，不另建 GC owner。

### Requirement: Plan history copying does not transfer execution authority
fork/导入新 session 或 rewind SHALL NOT 复制有效的旧审批关系、重新消费已结算决定或自动启动来源 handoff。可见正文不等于目标会话的执行资格；跨 session 实施 SHALL 以目标新提交和新审批建立关系。无法验证候选来源时 SHALL 拒绝执行，而非从 UI 历史重建授权。

#### Scenario: Fork or import a retained or approved plan
- **WHEN** 计划正文或控制历史复制到不同 session/workspace
- **THEN** 目标不复用来源 waiter/批准/handoff；若使用正文，按目标 owner 新提交并重新审批，不启动原工作。

#### Scenario: Rewind meets a consumed plan decision
- **WHEN** rewind 后旧批准窗口或已消费 handoff 再次出现
- **THEN** 旧操作不能重新获执行资格；候选保留/失效由当前 Control 与来源校验决定，展示回退不复活执行。

实施入口：SessionControl、`shell/src/session/fork.rs`、`actor/rewind.rs` 与现有 Timeline 分支恢复，不建立独立计划历史引擎。
