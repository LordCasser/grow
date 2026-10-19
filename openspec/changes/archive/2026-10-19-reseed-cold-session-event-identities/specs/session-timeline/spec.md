## ADDED Requirements

### Requirement: Cold writer identity recovery is independent of client replay

Cold session load SHALL seed the process event sequence above every valid ACP/Grow event identity in the loaded physical update history and every durable Timeline display expectation before publishing a writer actor or emitting new persistent events. Client noReplay, reconnect cursors, and rewind visibility SHALL NOT lower that bound. Exact append SHALL continue to reject different payloads that reuse an existing identity.

#### Scenario: Headless loads without replay

- **WHEN** a cold noReplay load finds an ordinary persisted update whose sequence exceeds all Timeline display expectations
- **THEN** new events use a sequence greater than that update and can be persisted without identity conflict.

#### Scenario: Historical row is hidden from replay

- **WHEN** rewind or a reconnect cursor excludes a physical historical row from client display
- **THEN** its valid event identity still contributes to the writer sequence lower bound.

#### Scenario: Display expectation precedes cache publication

- **WHEN** a durable Timeline expectation has no corresponding update-cache row
- **THEN** its identity also contributes to the sequence lower bound before writer publication.
