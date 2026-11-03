## MODIFIED Requirements

### Requirement: Subagent finalization retains one accountable owner

子任务执行返回、失败、panic 或取消后，系统 SHALL 保留该 child 的收尾责任，直到所需持久事实和交付责任已确认，或明确展示仍待修复。执行 outcome、提交状态和交付状态 SHALL 分离。未确认收尾不得被普通完成缓存淘汰、作为已完成依赖使用或用于绕过源实例仍存活时的 resume 拒绝。并发推进和暂存资源 SHALL 有界。Coordinator capacity accounting SHALL count each unique child identity still held by pending execution, active execution, pending finalization, or physical retirement. If one child has both a pending finalization and a physical retirement owner, it SHALL consume one capacity slot. Reaching the existing bound SHALL apply backpressure without discarding either responsibility; a slot becomes available only after the corresponding owners meet their existing completion conditions.

#### Scenario: Runner exits after output publication fails
- **WHEN** runner 已返回，但 output artifact 或 child result 提交失败
- **THEN** 查询显示执行已返回且提交受阻，保留同一 child 的收尾责任；不会把它静默移入可淘汰的完成项。

#### Scenario: Panic competes with cancellation
- **WHEN** runner panic 与取消或正常收尾竞争
- **THEN** 只有一个收尾 owner 推进原 child 的事实；已确认 outcome 不被覆盖，未确认状态仍可查询。

#### Scenario: Pending finalization reaches capacity
- **WHEN** 待收尾元数据或未持久正文达到容量上限
- **THEN** 新准入受到反压，超限正文保全失败可见，既有未确认责任不被当作成功完成或静默丢弃。

#### Scenario: Physical retirement reaches capacity
- **WHEN** children have completed execution but their physical retirement owners remain outstanding until the existing capacity is reached
- **THEN** another child is not dispatched, and all pending retirement and finalization responsibilities remain owned

#### Scenario: Retirement and finalization overlap
- **WHEN** one child simultaneously has a pending finalization and a physical retirement owner
- **THEN** its identity consumes one capacity slot rather than two

#### Scenario: Retiring owner is released
- **WHEN** a retiring owner is confirmed retired and a child identity is admitted afterward
- **THEN** capacity is available again, and the released owner is not joined or removed a second time

#### Scenario: Retiring identity remains unique after completed-cache eviction
- **WHEN** the completed presentation entry for a child is evicted while its physical retirement owner remains outstanding
- **THEN** a spawn reusing that child identity is rejected, and the existing retirement owner remains intact

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::finish_child/finish_panicked_child`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`。
