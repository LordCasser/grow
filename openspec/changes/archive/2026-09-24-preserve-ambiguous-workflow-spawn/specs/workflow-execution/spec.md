## ADDED Requirements

### Requirement: Ambiguous Workflow spawn preserves recovery authority
Workflow launch SHALL treat a durable Spawned acknowledgment loss or other uncertain Timeline write failure as an unknown commit outcome. It SHALL NOT erase the run source or publish a clear tombstone unless the Timeline proves Spawned was rejected before persistence. Recovery SHALL use the actual Timeline facts to decide whether the run exists.

#### Scenario: Spawned persisted but caller acknowledgment is lost
- **WHEN** the Timeline writer appends Spawned but launch loses its acknowledgment
- **THEN** launch reports the run identity and uncertainty, leaves source files available, and cold recovery can restore the run from the Spawned seed.

#### Scenario: Spawned was rejected before persistence
- **WHEN** Timeline validation rejects Spawned without attempting an append
- **THEN** launch may roll back the local run and tombstone its sidecar; no recoverable Spawned fact exists.

#### Scenario: No Spawned fact was committed
- **WHEN** a launch fails before Spawned or its uncertain append never became durable
- **THEN** cold recovery ignores any remaining source files because run discovery is based on Timeline Spawned facts.
