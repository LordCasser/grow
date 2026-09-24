## ADDED Requirements

### Requirement: Candidate preview staging does not retain candidate payloads
Transient session persistence staging SHALL retain at most the first candidate's ordering position and SHALL NOT retain candidate notification payloads. The canonical response projection is the only source of accepted candidate content. Untagged notifications interleaved before and after the candidate SHALL preserve their order around the projection.

#### Scenario: Fragmented candidates precede a canonical projection
- **WHEN** an attempt emits any number of candidate chunks interleaved with untagged notifications and commits a response projection
- **THEN** staging retains no candidate payload, the canonical response appears at the first candidate position, and untagged notifications retain their order around it.

#### Scenario: Candidate is discarded or never projected
- **WHEN** an attempt is discarded, superseded, or stopped without a response projection
- **THEN** no candidate payload is persisted, while staged untagged notifications retain their existing behavior and order.
