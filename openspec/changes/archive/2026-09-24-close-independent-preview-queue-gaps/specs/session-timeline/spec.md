## MODIFIED Requirements

### Requirement: Candidate preview uses a durable payload-free anchor

Transient session persistence SHALL retain neither candidate notification payloads nor an unbounded window of independent notifications. The first candidate SHALL create one durable, payload-free ordering anchor before canonical Timeline admission. During an active attempt, independent untagged ACP and one-shot Grow notifications emitted by the session actor SHALL be appended in arrival order with acknowledgement before another notification from that actor is delivered; previously buffered notifications SHALL be flushed first. A failed anchor or independent append SHALL prevent canonical admission. Live preview notifications from the session actor, including Grow notifications emitted during the attempt, SHALL have a per-session byte budget through gateway completion; exhaustion or an oversized single item SHALL fail the attempt's preview boundary before canonical admission. Replay SHALL suppress an anchor for a discarded attempt and SHALL insert admitted canonical content at that anchor before later independent updates. Live delivery MAY carry candidate notifications independently of persistence.

#### Scenario: Long interleaved attempt

- **WHEN** an attempt emits many candidate chunks interleaved with independent untagged ACP or one-shot Grow notifications
- **THEN** only one payload-free candidate anchor is persisted, earlier buffered updates are flushed first, independent updates stream to storage with acknowledgement rather than accumulating in the sender, and admitted replay places canonical content at the first candidate position.

#### Scenario: Preview persistence fails

- **WHEN** an anchor or independent append cannot be confirmed before response admission
- **THEN** the response is not admitted to the canonical Timeline, no candidate body enters replay, and the turn reports a projection-boundary failure.

#### Scenario: Discarded or interrupted attempt

- **WHEN** an attempt is discarded or stops without canonical admission
- **THEN** its anchor is suppressed in replay, while independently committed untagged notifications remain in their arrival order.

#### Scenario: Slow live gateway exhausts preview credits

- **WHEN** client completion stalls until the per-session preview byte budget is full, or one preview notification exceeds that budget
- **THEN** no additional candidate or Grow preview payload is enqueued to the gateway, the attempt records a preview-boundary failure, and canonical admission is refused without persisting candidate body text.

#### Scenario: Buffered update precedes an independent Grow notification

- **WHEN** a persistence writer holds an earlier merged ACP update while an active-attempt Grow notification arrives
- **THEN** it writes the earlier update first, confirms the Grow append before the actor can deliver another independent notification, and rejects canonical admission if either write fails.
