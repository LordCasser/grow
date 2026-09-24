## MODIFIED Requirements

### Requirement: Candidate preview uses a durable payload-free anchor

Transient session persistence SHALL retain neither candidate notification payloads nor an unbounded window of independent notifications. The first candidate SHALL create one durable, payload-free ordering anchor before canonical Timeline admission. During an active attempt, independent untagged notifications SHALL be appended in arrival order with acknowledgement before another notification from the same actor is delivered. A failed anchor or independent append SHALL prevent canonical admission. Live preview notifications SHALL have a per-session byte budget through gateway completion; exhaustion or an oversized single item SHALL fail the attempt's preview boundary before canonical admission. Replay SHALL suppress an anchor for a discarded attempt and SHALL insert admitted canonical content at that anchor before later independent updates. Live delivery MAY carry candidate notifications independently of persistence.

#### Scenario: Long interleaved attempt

- **WHEN** an attempt emits many candidate chunks interleaved with independent untagged notifications
- **THEN** only one payload-free candidate anchor is persisted, independent updates stream to storage with acknowledgement rather than accumulating in the sender, and admitted replay places canonical content at the first candidate position.

#### Scenario: Preview persistence fails

- **WHEN** an anchor or independent append cannot be confirmed before response admission
- **THEN** the response is not admitted to the canonical Timeline, no candidate body enters replay, and the turn reports a projection-boundary failure.

#### Scenario: Discarded or interrupted attempt

- **WHEN** an attempt is discarded or stops without canonical admission
- **THEN** its anchor is suppressed in replay, while independently committed untagged notifications remain in their arrival order.

#### Scenario: Slow live gateway exhausts preview credits

- **WHEN** client completion stalls until the per-session preview byte budget is full, or one preview notification exceeds that budget
- **THEN** no additional candidate payload is enqueued to the gateway, the attempt records a preview-boundary failure, and canonical admission is refused without persisting candidate body text.
