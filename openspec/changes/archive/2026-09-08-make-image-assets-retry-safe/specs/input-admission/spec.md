## ADDED Requirements

### Requirement: Image asset preparation is retry-safe
Repeated preparation of the same ordered normalized image batch SHALL reuse verified assets rather than allocate unbounded duplicate copies. Reused assets SHALL NOT enter a new batch rollback set. A lost durable-message acknowledgement SHALL NOT alone authorize removal of potentially referenced assets.

#### Scenario: Preparation retried
- **WHEN** an ordered image batch is prepared again with unchanged normalized bytes
- **THEN** its verified asset references remain stable.

#### Scenario: Acknowledgement unavailable
- **WHEN** image-bearing message commit acknowledgement is lost
- **THEN** referenced assets remain available pending authoritative reconciliation.

#### Scenario: Concurrent preparation
- **WHEN** two calls prepare the same ordered normalized batch concurrently
- **THEN** they return the same verified published paths and a losing call cleans only its own staging directory.

#### Scenario: Published bytes conflict
- **WHEN** an existing batch file differs from the expected normalized bytes
- **THEN** preparation fails without overwriting or removing the existing batch.
