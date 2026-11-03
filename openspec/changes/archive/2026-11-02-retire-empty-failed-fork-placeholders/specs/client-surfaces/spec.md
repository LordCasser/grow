## ADDED Requirements

### Requirement: Failed fork startup has a recoverable local destination

Initial fork failure SHALL retire an unbound empty placeholder and return to its surviving source only when that failed page is still foreground, preserving Dashboard attachment when applicable. A background failure SHALL NOT steal focus. Unsubmitted directives or drafts SHALL remain recoverable in an explicitly failed, closable draft view after startup/loading state clears. A late initial failure SHALL NOT overwrite a bound session. Worktree-fork startup failure SHALL use the same ownership rules. Failure cleanup SHALL NOT delete source history or assume an unacknowledged copy was never committed. Parent presentation SHALL distinguish a fork request from confirmed completion.

#### Scenario: Empty fork request fails
- **WHEN** an initial unbound fork fails without unsubmitted input
- **THEN** its placeholder retires and the foreground returns to the surviving source or a valid remaining surface.

#### Scenario: Unsent directive exists
- **WHEN** a failed fork owns an unsubmitted directive or draft
- **THEN** that content is retained without automatic execution, loading flags clear, and the failed draft can be closed through the normal draft checkpoint boundary.

#### Scenario: Failure is late
- **WHEN** the user is on another page or the original view has acquired a real binding
- **THEN** an empty background placeholder may retire without moving focus, and a bound replacement is preserved.

#### Scenario: Failed draft is removed from Dashboard
- **WHEN** a settled failed draft has no session id and its Dashboard removal is confirmed
- **THEN** the local view retires through draft checkpointing without sending a storage deletion.
