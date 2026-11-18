## ADDED Requirements

### Requirement: Autonomous model turns wait for context recovery
A typed context-window failure SHALL carry the failed effective window in its durable Turn terminal. Timeline replay and published working cuts SHALL preserve unresolved capacity evidence. Idle notification and Goal admission SHALL use the same recovery condition. Until a provider Turn has completed successfully or a new recovery basis exists and the effective input plus known output reservation fits, autonomous model turns SHALL remain parked. A larger effective window from an activated route and a committed history replacement for compaction, rewind, or rebuild are recovery bases. The normal full-request admission check SHALL still apply. Explicit model controls, user retries and manual compaction SHALL remain admissible through their usual boundaries.

#### Scenario: A notification arrives during the failed compaction
- **WHEN** compaction commits but the Turn still exceeds the active window, and a background receipt is pending or arrives afterward
- **THEN** the receipt stays pending while the actor accepts model controls, and it starts no new Turn or summary against the exhausted window.

#### Scenario: A new destination is committed
- **WHEN** a larger model is durably selected and activated, or a committed recovery operation changes the retained history
- **THEN** the next idle admission checks the new effective input plus known output reservation against the activated window before consuming a receipt or scheduling a Goal Turn.

#### Scenario: A destination is uncommitted or insufficient
- **WHEN** a larger candidate is rejected or superseded, the effective window remains unchanged, or even the larger destination cannot fit the projected request
- **THEN** the candidate does not release pending autonomous work, including when the original provider-reported failure was below the host's local estimate.

#### Scenario: Cold load with a pending receipt
- **WHEN** a session reloads with unresolved context-window failure evidence and pending durable receipts
- **THEN** restoring from the journal or a published working cut preserves the same idle admission decision and leaves receipts pending until recovery.

#### Scenario: An active Goal is blocked by context pressure
- **WHEN** an idle Goal has no admissible model continuation after a context-window failure
- **THEN** it starts no Turn and does not self-rearm idle scheduling until an external recovery edge.
