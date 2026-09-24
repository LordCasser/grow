## ADDED Requirements

### Requirement: Session sampler preview handoff bounds fragment backlog

A live session's sampler-to-Shell event handoff SHALL reserve capacity before forwarding each candidate text, reasoning, tool-argument, response-start, or reasoning-signature fragment. At most 4096 such fragments and approximately 64 MiB of charged fragment payload SHALL wait in the channel. The producer SHALL await capacity asynchronously so a slow session drainer backpressures the provider stream, and cancellation SHALL interrupt that wait. Receiving a fragment SHALL release its reservation before awaiting client or persistence delivery. Attempt ownership, chunk indexes, control-event order, and canonical admission SHALL remain unchanged.

#### Scenario: Session consumer stalls

- **WHEN** the session drainer stops receiving while an L2 stream keeps producing small or large fragments
- **THEN** the producer stops forwarding after the fragment credit budget is consumed; further fragments remain upstream rather than accumulating in the session event channel.

#### Scenario: Cancellation while waiting for credits

- **WHEN** an attempt is canceled after its fragment budget fills but before the drainer receives another fragment
- **THEN** the provider drive stops without forwarding the waiting fragment, and its attempt-discard terminal follows already-forwarded fragments in channel order.

#### Scenario: Drainer resumes

- **WHEN** the drainer receives queued fragments after a stall
- **THEN** their credits are returned, the producer may continue, and text/reasoning/tool fragment order and chunk indexes remain unchanged through the terminal event.
