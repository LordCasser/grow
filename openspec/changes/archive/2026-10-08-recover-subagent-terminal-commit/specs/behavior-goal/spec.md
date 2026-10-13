## ADDED Requirements

### Requirement: Late child finalization respects the original Goal owner

迟到或恢复的子任务结果及用量 SHALL 保留原 Goal identity 与 definition revision。只有当前 owner 与准入仍允许时才能触发自动消费或续跑；结果提交不得重新打开已停止或已换代的 Goal。失效 owner 的通知 SHALL 沿既有持久 dismiss 规则收敛，并保留可审计结果。

#### Scenario: Goal stopped before child receipt was admitted
- **WHEN** child result 恢复成功时其原 Goal 已暂停、阻塞、完成或预算关闭
- **THEN** 原结果和用量仍归属原 Goal，不自动重开准入或恢复模型工作。

#### Scenario: Goal definition changed during finalization
- **WHEN** 回执携带旧 definition revision，而当前 Goal 已采用新 revision
- **THEN** 回执不会作为新 revision 的依赖完成或续跑信号，按既有失效 owner 规则持久收敛。

证据与实施入口：`crates/codegen/shell/src/session/actor/notification_drain.rs`、`crates/codegen/shell/src/session/goal_tracker.rs`、`crates/codegen/shell/src/agent/subagent/mod.rs::admit_subagent_completion_receipt`。
