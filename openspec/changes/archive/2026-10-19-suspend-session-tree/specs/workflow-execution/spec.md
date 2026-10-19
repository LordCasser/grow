## ADDED Requirements

### Requirement: Session suspension preserves Workflow run and child bindings

会话挂起 SHALL 保留 Workflow Run identity、冻结脚本/参数/route、journal、原业务状态及未确认 child 的调用关联。执行实例因挂起停止 SHALL 不把 Run 写成永久 Interrupted/Cancelled，也不把挂起 child 写成成功 journal 值。恢复 SHALL 核对旧实例退出并关闭旧执行的持久结算，随后在同 Run 按既有 journal 推进执行轮次；原本 await_user、业务 pause 或 budget-limited 的 Run SHALL 保持原等待。

#### Scenario: Workflow waits for a suspended child
- **WHEN** Run 的 host 调用已关联 child，整树随后挂起
- **THEN** 恢复找到原 child 和 pending operation，重新等待或消费其 canonical 结果，不对该逻辑调用重新 spawn child。

#### Scenario: Workflow was already waiting for the user
- **WHEN** Run 原本因 await_user 或显式业务 pause 停止
- **THEN** session resume 恢复其可见状态与交互，不越过原业务等待自动执行下一阶段。

#### Scenario: Old execution reports a late result
- **WHEN** 原执行实例的结果在挂起或新执行轮次接管后到达
- **THEN** 按原 Run/operation/child 身份核对并只接纳一次，不唤醒退役执行或把结果误交另一调用。

#### Scenario: Workflow restoration fails before the original Turn is released
- **WHEN** Session runtime 已提交 `ExecutionReleased`，但本地 active Workflow 尚未成功恢复
- **THEN** 原 Turn 和会话执行 gate 保持关闭；同一 restore owner 可重试尚未恢复的原 Run，已成功恢复的 Run 不重复启动。

#### Scenario: Process restarts after runtime release with an unstarted suspended Workflow
- **WHEN** root Runtime 已持久进入 Running，但某个原 Run 仍是 `execution_suspended` 且 Active
- **THEN** 冷加载将该同一 Run 明确投影为可由 `ControlRun Resume` 恢复的 `InfraPaused`，保留 journal 和未决 child 身份，不自动重放 Workflow；不得将其留作永久 Active+suspended。

目标入口：`crates/codegen/shell/src/session/workflow/{host_service,store}.rs`、`crates/codegen/workflow/src/journal.rs` 及 Workflow execution terminal watcher。
