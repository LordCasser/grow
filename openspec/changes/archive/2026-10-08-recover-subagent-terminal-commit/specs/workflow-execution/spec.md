## ADDED Requirements

### Requirement: Workflow child waits preserve unconfirmed finalization

Workflow child 调用在执行或收尾结果不明时 SHALL 保留原 Run、调用与 child 的关联，报告等待或恢复受阻，不将其写为可重放的最终成功。恢复 SHALL 先核对原 child 的 canonical 结果；没有可验证关联时不得自动重新派发。迟到结果不能重启已经结束或被替换的 Run execution。

#### Scenario: Child wait returns before terminal commit
- **WHEN** child 等待期限到达而 parent Ended 尚未确认
- **THEN** Workflow 保留未确认调用及原 child identity，不把该状态作为 child 成功值重放或启动依赖步骤。

#### Scenario: Resume encounters an unconfirmed child call
- **WHEN** 恢复的 Workflow 遇到先前已派发但未确认结果的调用
- **THEN** 先核对原 child；可验证结果按既有 journal identity 接纳，证据不足则停止自动推进，不为同一逻辑调用生成另一 child。

#### Scenario: Late child belongs to a retired execution
- **WHEN** 子任务最终结果到达时原 Run execution 已终止或换代
- **THEN** 保留结果关联而不自动恢复旧 execution 或将结果交给新调用。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`、`crates/codegen/workflow/src/journal.rs`、`crates/codegen/shell/src/session/workflow/`。
