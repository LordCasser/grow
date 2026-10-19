## ADDED Requirements

### Requirement: Live turn terminals classify stop reasons explicitly
Pager SHALL treat only known successful terminal stop reasons as successful completion. Unknown or missing stop reasons SHALL NOT produce a completion marker or success notification; they SHALL be surfaced as a failure. Existing cancellation, provider error, and rate-limit handling remains distinct.

#### Scenario: Unknown or interrupted stop reason
- **WHEN** a live durable terminal has an unknown reason or `interrupted`
- **THEN** Pager finalizes the matching prompt as a failure and emits no success notification.

#### Scenario: Known successful stop reason
- **WHEN** a live durable terminal has `end_turn`, `max_tokens`, `max_turn_requests`, or `refusal`
- **THEN** Pager emits the existing completion behavior.

#### Scenario: Duplicate terminal
- **WHEN** a terminal for the already-finalized prompt is delivered again
- **THEN** it does not add another marker or repeat teardown/notification.
