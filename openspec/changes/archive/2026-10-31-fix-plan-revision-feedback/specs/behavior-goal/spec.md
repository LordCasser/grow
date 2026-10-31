## ADDED Requirements

### Requirement: Plan revision context carries the accepted review feedback
Plan 要求修改的决定被持久接纳后，下一次模型可见的 PlanPhase 上下文 SHALL 同时包含当前计划正文与该决定的非空反馈。Drafting 和 Amending SHALL 遵守同一规则；恢复使用同一已接纳状态，不把旧提交的反馈当作新提交的反馈。修订不授予执行权限。

#### Scenario: Initial plan is returned for revision
- **WHEN** 用户对首次待审批计划提交非空修订反馈，计划回到 Drafting
- **THEN** 模型的 PlanPhase 上下文包含原计划和完整反馈，要求重新 submit，仍禁止执行。

#### Scenario: Amendment is returned for revision
- **WHEN** 用户对执行中提交的替代计划要求修改
- **THEN** Amending 上下文包含替代计划与当前反馈，要求重新 amend，仍禁止执行。

#### Scenario: Revision is restored or repeated
- **WHEN** 修订状态从持久 Control 恢复，或新计划再次被要求修改
- **THEN** 当前上下文保留对应提交的反馈；新的修订反馈替代旧反馈，不能遗漏或串用。

#### Scenario: Empty feedback or replacement submission
- **WHEN** 要求修改不含非空反馈，或修订后的新 artifact 已提交审批
- **THEN** 当前 PlanPhase 上下文不虚构反馈、不沿用上一提交的反馈；新 artifact 等待自己的审批。
