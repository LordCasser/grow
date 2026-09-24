## ADDED Requirements

### Requirement: Oversized sampler preview events fail before enqueue

A live session sampler handoff SHALL reject an individual charged preview fragment whose payload exceeds its 64 MiB credit capacity. It SHALL fail the current attempt locally before acquiring credits or forwarding the event, rather than charging a capped cost for an oversized item. Control and terminal events SHALL retain their existing uncharged path.

#### Scenario: One fragment exceeds all credits

- **WHEN** a text, reasoning, tool-argument, response-start or signature fragment has more charged bytes than the per-session handoff capacity
- **THEN** the producer fails the attempt without enqueuing that fragment, and no downstream credit accounting is corrupted.

#### Scenario: Fragment exactly fits all credits

- **WHEN** a charged fragment exactly equals the handoff capacity
- **THEN** it can reserve all credits and proceed under the ordinary acknowledgement fence.
