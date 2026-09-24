## ADDED Requirements

### Requirement: Behavior availability projections follow admission changes
The Shell SHALL publish a versioned Behavior availability projection when a foreground transition or pending step-control change alters Behavior admission while the session remains available. The projection revision SHALL be allocated before reading its inputs and SHALL increase for every newly built projection. Pager SHALL accept a projection only when its revision is newer than the latest accepted revision, so a delayed older projection cannot replace newer availability.

#### Scenario: Prompt promotion refreshes an open Behavior picker
- **WHEN** a queued prompt is promoted from an idle session into a regular foreground turn while the client is attached
- **THEN** the Shell publishes the availability assessed from that foreground state before starting the turn, and Pager refreshes any open settings snapshot from the new projection

#### Scenario: Goal continuation refreshes Behavior availability
- **WHEN** idle arbitration promotes an Active Goal continuation into a regular foreground turn
- **THEN** the Shell publishes the availability assessed from that foreground state before the continuation starts

#### Scenario: Foreground enters terminal settlement
- **WHEN** a regular foreground enters Settling for its durable terminal transaction
- **THEN** the Shell publishes the availability assessed from the busy settlement state without holding the admission lock during publication

#### Scenario: Turn settlement returns the session to idle
- **WHEN** a regular turn has settled and completion arbitration admits no successor foreground work
- **THEN** the Shell publishes availability assessed from idle admission facts

#### Scenario: Manual compaction owns and releases foreground
- **WHEN** manual compaction takes an idle foreground and later completes without admitting successor work
- **THEN** the Shell publishes its busy projection at admission and its idle projection after completion arbitration

#### Scenario: Pending step control changes Behavior admission
- **WHEN** a model, Agent, or Goal step control is admitted or its queue is drained
- **THEN** the Shell publishes a projection after releasing the admission lock, and Pager displays whether Behavior selection may overtake the pending control

#### Scenario: Older projection arrives late
- **WHEN** Pager has accepted a Behavior availability projection with a newer revision and then receives an older revision
- **THEN** Pager retains the newer projection and its visible availability
