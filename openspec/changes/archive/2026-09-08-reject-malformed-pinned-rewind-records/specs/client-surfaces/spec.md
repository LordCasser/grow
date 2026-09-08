## ADDED Requirements

### Requirement: Pinned rewind parsing rejects malformed records
Pinned rewind JSONL readers SHALL reject a nonblank record that fails deserialization, returning InvalidData with its one-based physical line number rather than a successful partial record set. Historical load failure SHALL retain the source and live points for retry.

#### Scenario: Invalid record among valid records
- **WHEN** a pinned history contains valid records surrounding an invalid record
- **THEN** the reader fails at that record and no valid prefix is merged into the tracker.

#### Scenario: Retry after repair
- **WHEN** that pinned file is repaired
- **THEN** the next full load succeeds and retains existing live-point precedence.

#### Scenario: Non-record whitespace
- **WHEN** a pinned history is empty or includes blank lines and otherwise valid records
- **THEN** blank lines remain accepted and count toward diagnostic line numbers.

#### Scenario: Metadata parse failure
- **WHEN** the pinned metadata reader encounters a record it cannot deserialize
- **THEN** the existing picker fallback uses in-memory points without consuming the historical source.
