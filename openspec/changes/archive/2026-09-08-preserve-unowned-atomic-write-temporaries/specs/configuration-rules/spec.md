## ADDED Requirements

### Requirement: Atomic state writers preserve unowned temporary paths
The shared atomic state writer SHALL leave a preexisting temporary path untouched when exclusive creation fails. Cleanup after write or publication failure SHALL apply only after this attempt has successfully created its temporary file. Successful replacement SHALL preserve the requested Unix creation-mode behavior.

#### Scenario: Temporary name collision
- **WHEN** exclusive creation fails because the temporary path already exists
- **THEN** both that path and the existing destination remain unchanged

#### Scenario: Publication fails after creation
- **WHEN** this attempt creates a temporary file but cannot publish it
- **THEN** its temporary file is cleaned up while the destination remains intact
