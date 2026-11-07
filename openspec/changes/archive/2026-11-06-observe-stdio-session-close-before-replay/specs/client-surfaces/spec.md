## ADDED Requirements

### Requirement: Stdio bridge observes closed sessions before reconnect replay

The CLI stdio bridge SHALL deliver each nonempty outgoing message to its existing replay-cache parser before forwarding. The parser SHALL apply its supported session close operations to the same replay owner as initialize/new/load and SHALL leave unrelated methods unchanged. Closed sessions SHALL be excluded from later reconnect loads and primary restored-session selection; other cached sessions SHALL retain their deterministic replay order.

#### Scenario: Closed session is excluded through the real forwarding path

- **WHEN** the client forwards initialize, loads two sessions, closes one with a supported close method and reconnects to a replacement leader
- **THEN** the replacement receives no load for the closed session, receives one load for the remaining session and reports an open primary restored session.

#### Scenario: Unrelated content contains lifecycle text

- **WHEN** an unrelated outgoing method includes lifecycle method text in its payload
- **THEN** the message is forwarded without mutating the replay cache according to payload text.
