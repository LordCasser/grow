## ADDED Requirements

### Requirement: Parent-child inquiries use isolated Sideband execution
A running child SHALL be able to ask its immediate parent a question, and a primary agent SHALL be able to ask a directly-owned running child. Runtime-derived identities SHALL authorize the relationship. Questions SHALL use asynchronous tool-free Sideband execution without entering or interrupting the target foreground context. Children SHALL NOT gain cross-session discovery/inquiry or upward intervention.

#### Scenario: Child clarification during parent work
- **WHEN** a child asks its parent while the parent foreground is busy or waiting for that child
- **THEN** a Sideband answers from frozen parent context, returns the answer to the child, and the main view shows one correlated inquiry row through receipt and completion.

#### Scenario: Parent asks child
- **WHEN** the parent asks its running child a question
- **THEN** the child answers via Sideband without changing its foreground task or requiring cross-workspace approval for its delegated worktree.

#### Scenario: Invalid route or cancellation
- **WHEN** a caller targets an unrelated, sibling, terminated or Workflow-owned hidden child, or cancels an in-flight inquiry
- **THEN** a typed local failure/cancellation is returned without injecting user input, leaking another session's context or pausing the Goal.

### Requirement: Parent intervention has explicit delivery timing
A primary agent SHALL be able to send a message to a directly-owned running child with an explicit immediate-interrupt or queued-next-step option. The message SHALL be attributed to the parent and durably received before acknowledgement. Retries and restoration SHALL preserve exactly-once inbox consumption.

#### Scenario: Queued intervention
- **WHEN** the parent sends a non-interrupting message
- **THEN** the child completes its current work boundary and includes the message at the next step's sampling without cancelling the active request.

#### Scenario: Immediate intervention
- **WHEN** the parent requests immediate interruption
- **THEN** the child safely preempts the current model request or interruptible wait and resamples with the message, preserving non-interruptible tool outcomes, Timeline integrity and the current Goal lifecycle.

#### Scenario: Restore or retry
- **WHEN** an acknowledged intervention is retried or its receiving session restores
- **THEN** the same receipt is not duplicated and unconsumed context remains available without impersonating human input.
