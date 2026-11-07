## ADDED Requirements

### Requirement: Parent sampling tools show dispatch completion in place

Pager SHALL display a parent `set_subagent_model` call using one compact tool row whose state changes in place from dispatching to queued or failure. The row SHALL retain the target and requested model and/or effort. A queued result SHALL mean target actor admission only and SHALL NOT claim that the new selection is already applied. Detailed and raw views SHALL retain the full arguments and result.

#### Scenario: A busy child admits a selection

- **WHEN** a parent sends a model or effort selection and the busy child admits it before its current Step closes
- **THEN** the same tool row changes from dispatching to queued without a second completion row or an applied claim.

#### Scenario: Dispatch is rejected

- **WHEN** the target or selection is rejected
- **THEN** the original tool row shows failure and preserves the error rather than remaining in dispatching or showing queued.

#### Scenario: Completion is observed again

- **WHEN** the same tool completion is repeated or restored within its owning session
- **THEN** Pager retains one completed row for that call and preserves its target and requested fields.

### Requirement: Child sampling tools retain ordinary control feedback

A host-authorized parent tool selecting child sampling SHALL enter the child's existing Sampling control feedback and durable receipt path using a host-owned operation identity. Child UI SHALL show the admitted pending control and the ordinary terminal control notice when the effective selection changes. The control SHALL remain distinct from user input, and a queued parent tool result SHALL NOT establish application. Existing closed-Step application, no-op, supersession, failure and hydration behavior SHALL remain authoritative.

#### Scenario: Selection applies after a Step

- **WHEN** a child receives an authorized parent selection while a Step is running
- **THEN** the child shows pending control while its current selection remains unchanged, and after that Step closes it records the ordinary applied receipt and displays one Sampling control notice.

#### Scenario: Initial child controls precede opening the child view

- **WHEN** the parent receives queued before the user opens its running child and child startup crosses asynchronous parent lifecycle publication
- **THEN** the child initial control authority is published after its Spawned route is established and before its first prompt or live Pending, so opening the child retains the ordinary pending display without accepting an unknown actor epoch.

#### Scenario: Effort is selected alone

- **WHEN** the parent changes only reasoning effort
- **THEN** the child composes it with its latest desired model, displays the effective applied selection and does not restore a stale model.

#### Scenario: No change or superseded control

- **WHEN** the selection is a no-op or a newer admitted control supersedes it
- **THEN** existing terminal receipt and no-op/supersession presentation semantics are retained without claiming a switch that did not occur.

#### Scenario: Application cannot be durably confirmed

- **WHEN** the selection's terminal control event fails persistence
- **THEN** the existing finish path reports the failure and does not synthesize a durable applied notice, even if the parent earlier received queued.

#### Scenario: Hydration and recovery

- **WHEN** a session merely restores or reloads its persisted model selection
- **THEN** it does not manufacture a parent tool control, a user prompt or a new switch notice.
