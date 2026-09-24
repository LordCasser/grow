## ADDED Requirements

### Requirement: Auxiliary provider attempts enter the owning session usage projection

Session Timeline SHALL durably record known or unknown usage for each admitted main and Sideband provider attempt, each child's final usage bill and session-level incomplete facts. Sideband attempt identity SHALL be `(sideband_id, attempt_no)` in its owning session; each attempt SHALL be counted at most once, including failed and retried requests. A cold recovery with an admitted Sideband attempt lacking terminal usage SHALL retain an incomplete lower-bound ledger. Recovery SHALL reject malformed or conflicting known billing facts before publishing an actor. A child Sideband SHALL be charged to its child ledger and reach the parent only through the child's final bill.

#### Scenario: Failed attempt followed by a successful retry

- **WHEN** one Sideband issues two provider requests under successive attempt numbers, the first fails with known usage and the second succeeds
- **THEN** the owning session records both usages once under the selected route model, while the Sideband Result is not billed again

#### Scenario: Cancellation or crash after admission

- **WHEN** a Sideband provider request has been admitted but usage cannot be confirmed before owner cancellation or cold recovery
- **THEN** the owning session retains an incomplete usage lower bound rather than an exact zero, without fabricating token counts

#### Scenario: Duplicate and conflicting Sideband bills

- **WHEN** a Sideband attempt settlement is repeated with an identical payload or with a different payload
- **THEN** the identical payload is a no-op and the conflicting payload fails closed in live and restored projections

#### Scenario: Child Sideband final bill

- **WHEN** a child consumes tokens in its main loop and a Sideband, then settles its final bill to its parent
- **THEN** the child ledger includes both attempts and the parent includes their aggregate once under that child identity
