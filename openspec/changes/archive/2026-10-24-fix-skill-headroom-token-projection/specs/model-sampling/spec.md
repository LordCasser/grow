## ADDED Requirements

### Requirement: Skill publication uses projected request context pressure

User and model skill publication SHALL check existing context using ChatState's current provider-anchored request projection, including committed Surface deltas and the current request envelope adjustment. The commit SHALL add the estimate of all complete new user/result/skill items and recheck the current Surface revision and reserved headroom. Raw history omitted by provider projection SHALL NOT independently cause rejection; known provider overhead SHALL NOT be discarded. The dependent request's final projection and budget validation SHALL remain required.

#### Scenario: Raw history exceeds projected request size

- **WHEN** raw history includes reasoning omitted by the request projection and the current projected context plus a complete skill group fits the reserved budget
- **THEN** the group may publish atomically without rejecting solely because the raw Surface estimate is larger.

#### Scenario: Provider accounting exhausts available space

- **WHEN** the current provider anchor plus complete new skill content exceeds reserved headroom, even without a Surface revision change
- **THEN** user publication rejects without consuming or exposing any part of the group, and model publication uses its existing paired rejection result without publishing skill guidance.
