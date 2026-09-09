## ADDED Requirements

### Requirement: Paste collection preserves input-batch boundaries
A detected unbracketed multiline paste SHALL remain one pending insertion across bounded input collection passes. Reaching an event budget SHALL yield without committing the paste prefix. The existing idle boundary SHALL flush the complete insertion even without a subsequent input event.

#### Scenario: Large paste exceeds collection budget
- **WHEN** one continuous multiline paste exceeds the input extension event budget and ends with an unterminated line
- **THEN** its complete text is inserted once as one paste, without separately routed tail keystrokes or multiple chips.

#### Scenario: Exactly exhausted batch becomes idle
- **WHEN** collection reaches its event budget and no further input arrives
- **THEN** the idle deadline flushes the pending paste without requiring another key.

#### Scenario: Event-loop fairness
- **WHEN** a large paste is still arriving
- **THEN** each collection pass retains its bounded event budget and returns to the main loop.

#### Scenario: Ordinary input remains ordinary
- **WHEN** input is a normal key, a completed bracketed paste, or a non-paste event storm
- **THEN** existing key routing and bounded handling remain unchanged.

#### Scenario: Control key interrupts pending collection
- **WHEN** a control key arrives during detection or at the next pending-paste batch
- **THEN** collection returns without waiting for the remaining paste tail and retains the control key for normal routing.
