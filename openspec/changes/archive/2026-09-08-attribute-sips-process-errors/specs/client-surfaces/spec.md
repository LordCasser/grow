## ADDED Requirements

### Requirement: Sips process failures identify their stage
Sips process errors SHALL identify startup, group attachment, wait or cleanup stages while retaining the original error kind and diagnostic text.

#### Scenario: Process startup fails
- **WHEN** spawning or its detach/pre-exec hook fails
- **THEN** report startup-stage context with the underlying error rather than an unattributed OS error.

#### Scenario: Process ownership or cleanup fails
- **WHEN** attachment, waiting or cleanup reports an error
- **THEN** include the failing stage without suppressing errors or changing existing recovery behavior.
