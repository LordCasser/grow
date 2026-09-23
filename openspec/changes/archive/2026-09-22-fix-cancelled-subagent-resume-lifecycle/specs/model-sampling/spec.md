## ADDED Requirements

### Requirement: Subagent cancellation settles admitted attempts before closing the child Timeline

Subagent cancellation SHALL stop new provider admission, cancel active provider work and await the terminal evidence and all applicable usage settlement for every admitted attempt before committing the SubagentResult that closes the child Timeline. Unknown usage SHALL cross the existing durable incomplete-settlement boundary rather than being treated as zero. A cancellation signal alone SHALL NOT authorize child Timeline closure.

#### Scenario: Cancellation overlaps final attempt settlement

- **WHEN** a child is cancelled after its final provider attempt was admitted but that attempt's evidence or an applicable usage settlement acknowledgment is still pending
- **THEN** the cancellation remains in settlement, the child SubagentResult and parent Ended reference remain uncommitted, and the attempt settlement is allowed to reach its durable boundary before child closure.

#### Scenario: Settlement completes after cancellation

- **WHEN** all admitted attempt evidence and known or incomplete usage settlements are durably acknowledged and the child turn reaches its cancellation terminal
- **THEN** the system may commit exactly one SubagentResult(cancelled), followed by the parent Ended fact that references it, and no later attempt event is appended to that child Timeline.

#### Scenario: Settlement or terminal acknowledgment fails

- **WHEN** the child cannot confirm attempt settlement, its turn terminal, or the required persistence frontier within the existing bounded shutdown policy
- **THEN** the system reports a terminalization failure, does not commit a misleading canonical SubagentResult and does not make the lifecycle eligible for resume.

#### Scenario: Cancellation has no admitted provider work

- **WHEN** a child is cancelled before any provider attempt is admitted
- **THEN** the empty settlement frontier is acknowledged through the same terminal path and the canonical cancelled lifecycle may close without a fixed delay.
