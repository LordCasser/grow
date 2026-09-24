## MODIFIED Requirements

### Requirement: Session sampler preview handoff bounds fragment backlog

A live session's sampler-to-Shell event handoff SHALL reserve capacity before forwarding each candidate text, reasoning, tool-argument, response-start, or reasoning-signature fragment. At most 4096 such fragments and approximately 64 MiB of charged fragment payload SHALL be pending across the sampler and session-event channels. The producer SHALL await capacity asynchronously so a slow session actor backpressures the provider stream, and cancellation SHALL interrupt that wait. A fragment's reservation SHALL be returned only after its translated events have been consumed from the session event FIFO. A closed downstream consumer SHALL release blocked producers. The ReplayBuffer SHALL retain at most one pending notification with a coalescing threshold no greater than the fragment byte budget. Attempt ownership, chunk indexes, control-event order, and canonical admission SHALL remain unchanged.

#### Scenario: Session consumer stalls

- **WHEN** the session actor stops consuming events while the sampler drainer continues receiving L2 fragments
- **THEN** the drainer retains credit for the unacknowledged fragment, the producer stops forwarding once its remaining credit budget fills, and the second session event channel does not accumulate additional candidate payloads without bound.

#### Scenario: Cancellation while waiting for credits

- **WHEN** an attempt is canceled after its fragment budget fills but before the actor acknowledges another fragment
- **THEN** the provider drive stops without forwarding the waiting fragment, and its attempt-discard terminal follows already-forwarded fragments in channel order.

#### Scenario: Downstream actor closes

- **WHEN** the session actor closes before acknowledging a translated fragment
- **THEN** the drainer closes the credit budget and sampler receiver, so a producer waiting for capacity exits without retaining the payload indefinitely.

#### Scenario: Drainer resumes

- **WHEN** the actor consumes the queued translated notification and its acknowledgement fence
- **THEN** the corresponding credits are returned, the producer may continue, and text/reasoning/tool fragment order and chunk indexes remain unchanged through the terminal event.
