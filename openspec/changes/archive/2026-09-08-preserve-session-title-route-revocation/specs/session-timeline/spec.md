## ADDED Requirements

### Requirement: Manual title route revocation survives background failure
Revoking automatic title generation for a manual title command SHALL remain effective when an already-running title worker later fails. Such a worker SHALL NOT re-enable automatic generation for subsequent input. Ordinary transient failures without revocation SHALL retain retry eligibility.

#### Scenario: Manual rename during an in-flight title attempt
- **WHEN** a title worker owns the route, a manual title command revokes it, and the worker later encounters a retryable setup or persistence failure
- **THEN** subsequent input cannot claim the old automatic title route

#### Scenario: Transient failure without manual revocation
- **WHEN** title setup fails while its ownership remains valid
- **THEN** the route can be restored for a later eligible input without creating simultaneous owners
