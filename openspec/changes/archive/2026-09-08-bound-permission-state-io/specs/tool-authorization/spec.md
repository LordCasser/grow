## ADDED Requirements

### Requirement: Permission cache IO is bounded
Permission cache reads SHALL admit only ordinary files of at most 1 MiB, checking metadata on the opened handle and limiting actual bytes before UTF-8 and TOML parsing. Rejected reads SHALL preserve the source and use default remembered state without shared fallback. Unix opening SHALL not wait for a FIFO writer. Serialized writes over 1 MiB SHALL fail before atomic publication, preserving the previous destination.

#### Scenario: Exact and excessive bytes
- **WHEN** valid serialized state is exactly 1 MiB or a source exceeds that limit
- **THEN** the exact-boundary state loads and the excessive source is rejected; actual read consumes at most limit+1 bytes.

#### Scenario: Special source
- **WHEN** a Unix permission cache is a FIFO without a writer
- **THEN** reading rejects it without waiting for a writer and preserves the FIFO.

#### Scenario: Oversized write
- **WHEN** a serialized permission snapshot exceeds 1 MiB
- **THEN** writing returns an error and leaves the prior destination bytes unchanged.
