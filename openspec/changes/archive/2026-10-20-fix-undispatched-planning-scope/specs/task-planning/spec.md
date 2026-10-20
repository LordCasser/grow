## ADDED Requirements

### Requirement: Scope switches accept explicitly disposed dispatch failures

切换工作范围时，actor SHALL 使用 `LocalPlan::validate_transition_from` 的既有 transition 规则判断旧范围义务。明确记录的未派发事实可处置可选 link；必需 link 还必须有带理由的 Released review。活动或结果未知的 link、等待、未处理 replan，以及未处置的必需交付 SHALL 继续阻止切换。失败不得将执行伪造为 terminal 或生成结果。

#### Scenario: Optional dispatch failure is disposed without review
- **WHEN** 可选 child 明确未派发且没有等待或其他旧范围义务
- **THEN** 无需交付 review 即可切换 scope，link 仍保留为非 terminal 且无结果的历史事实。

#### Scenario: Required dispatch failure is released
- **WHEN** 必需 child 明确未派发并通过带理由的 Released review
- **THEN** 可以切换 scope，未派发事实不被改写为完成结果。

#### Scenario: Required dispatch failure is not released
- **WHEN** 必需 child 明确未派发但没有 Released review
- **THEN** scope switch 被拒绝，旧计划保持不变。

#### Scenario: Active or unknown execution is not disposed
- **WHEN** link 没有明确 dispatch failure，且仍活动或结果未知
- **THEN** scope switch 被拒绝，link 保持非 terminal 且无伪造结果。
