## ADDED Requirements

### Requirement: Bulk replay avoids cumulative lifecycle copying
Timeline restoration SHALL validate all events and expose only a fully validated fold, without copying all accumulated lifecycle history for every replayed event. Live prepare and accept SHALL retain failed-write atomicity.

#### Scenario: Long valid history
- **WHEN** a long Timeline is restored in bulk
- **THEN** Surface, event sequence, lifecycle ownership and pending Control activation match transactional event acceptance.

#### Scenario: Invalid history
- **WHEN** an event violates content or lifecycle constraints during bulk replay
- **THEN** restoration fails without exposing a partially restored Timeline or modifying persisted session data.

#### Scenario: Invalid live append
- **WHEN** live prepare or accept rejects an event
- **THEN** the previously accepted Timeline remains unchanged.
