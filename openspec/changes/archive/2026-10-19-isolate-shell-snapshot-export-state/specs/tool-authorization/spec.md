## ADDED Requirements

### Requirement: Persistent Bash snapshots isolate serializer export state
Persistent Bash state serialization SHALL retain existing user-export and function persistence semantics and preserve the original allexport option across commands without automatically exporting serializer payload variables. Snapshot replay SHALL restore payloads before re-enabling the original allexport flag while retaining the other POSIX/shopt ordering. Serialization SHALL NOT change tool authorization or replace the actual user command exit status.

#### Scenario: Snapshot a large function with allexport enabled
- **WHEN** a persistent Bash command enables allexport while a large unexported function is present
- **THEN** serialization and the next command complete without exporting the large intermediate payload; the function and user-exported values remain available and allexport is restored.

#### Scenario: Allexport is disabled
- **WHEN** the user's persistent Bash state has allexport disabled
- **THEN** replay leaves it disabled while retaining the existing function, variable and command-result behavior.

#### Scenario: Serializer payload names already have exported user values
- **WHEN** the user has exported a variable sharing a payload-local name, such as all_functions or content
- **THEN** the local payload is explicitly unexported before capture and encoding; the user's captured exported value survives replay rather than being replaced by the large serializer payload.

#### Scenario: POSIX mode has an explicitly disabled implied shopt
- **WHEN** supported Bash state combines POSIX mode with inherit_errexit explicitly disabled
- **THEN** replay retains that combination by restoring POSIX mode before the captured shopt values.
