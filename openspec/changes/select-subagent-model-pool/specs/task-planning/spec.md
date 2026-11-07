## ADDED Requirements

### Requirement: Session model pools constrain autonomous delegation

A root session SHALL own a user-selected pool of additional canonical models for ordinary subagent tool delegation. An empty pool SHALL permit only the root's currently selected model. The pool SHALL NOT store reasoning effort; the Agent MAY choose an effort supported by the actual selected model. Effective additional models SHALL remain bounded by the existing selectable catalog and authorization policy. Ordinary descendants SHALL NOT expand the root pool.

#### Scenario: Default pool and effort

- **WHEN** the user has selected no additional subagent models
- **THEN** a new delegated execution may use only the current root model, with any effort that model supports rather than requiring the root's current effort.

#### Scenario: Pool changes during a Step

- **WHEN** the user submits a pool change while a model Step is active
- **THEN** the current Step retains its admitted work, and the new pool is atomically committed and applied at the next existing closed-Step control boundary.

#### Scenario: Model-visible and enforced revisions agree

- **WHEN** a pool revision becomes effective
- **THEN** the next normal model request receives its current full pool and superseding reminder, tool model guidance uses the same revision, and fresh tool admission rejects model selections outside that pool with the current version and allowed selections.

#### Scenario: Existing child and new authorization

- **WHEN** an already-running child's model is removed from the pool
- **THEN** its already-admitted work may finish, while a new model selection or new execution resume must satisfy the current pool; effort-only changes do not grant another model.

#### Scenario: Recovery and conflicting edits

- **WHEN** the session reconnects, cold loads or compacts, or another client submits a stale based-on revision
- **THEN** recovery retains the latest effective pool and model context, and the stale edit is rejected with the authoritative snapshot rather than overwriting a newer selection.
