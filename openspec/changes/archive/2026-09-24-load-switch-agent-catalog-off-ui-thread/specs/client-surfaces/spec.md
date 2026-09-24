## ADDED Requirements

### Requirement: Switch Agent discovery does not block Pager input

Pager SHALL construct a prompt and open the `/agent` picker without filesystem-backed Agent discovery on the UI event path. The picker SHALL immediately offer built-in Agents and refresh from a bounded background scan of normal and plugin definitions. A failed or timed-out scan SHALL preserve the valid built-in choices; a late result SHALL apply only to the Agent view, session binding, and picker request that initiated it. Workflow Run children SHALL use their frozen Agent snapshot without a live scan.

#### Scenario: Filesystem-backed discovery stalls
- **WHEN** the user opens an Agent view or `/agent` picker while definition discovery remains blocked
- **THEN** prompt creation and picker input remain responsive, built-in choices remain selectable, and the request reports a finite failure after its deadline while the worker retains its permit until exit.

#### Scenario: Picker changes before discovery returns
- **WHEN** the user closes or replaces the picker, changes the Agent view's session binding, or opens a newer picker before an older scan finishes
- **THEN** the old result does not reopen or replace the current picker or catalog.

#### Scenario: Workflow child opens Agent picker
- **WHEN** a Workflow Run child has frozen Agent names
- **THEN** its picker lists exactly that snapshot and starts no live discovery.
