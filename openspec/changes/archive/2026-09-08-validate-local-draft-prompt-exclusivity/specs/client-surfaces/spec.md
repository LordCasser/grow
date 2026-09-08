## ADDED Requirements

### Requirement: Local draft records contain at most one prompt source
Local draft validation SHALL reject records containing both composer and staged_prompt, including empty values. Invalid writes SHALL not replace or remove an existing record; invalid loaded records SHALL follow quarantine policy and SHALL not restore either prompt or its deferred behavior.

#### Scenario: Two prompt sources on disk
- **WHEN** a record contains both composer and staged_prompt
- **THEN** loading quarantines it instead of selecting one prompt silently

#### Scenario: Conflicting write has no payload
- **WHEN** both prompt fields are present but empty
- **THEN** validation rejects the write before the empty-record deletion path

#### Scenario: One supported prompt source
- **WHEN** only composer or only staged_prompt is present and otherwise valid
- **THEN** existing draft persistence and local-only recovery remain available
