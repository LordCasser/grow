## ADDED Requirements

### Requirement: Unified log records have a bounded complete encoding
Unified log writers SHALL encode each JSONL record, including its terminating LF, within 65,536 bytes. Serialization SHALL stop before appending bytes that exceed this budget. An oversized record SHALL be replaced by a complete, valid diagnostic record identifying the omitted entry and the byte limit; no prefix of the rejected encoding SHALL be written.

#### Scenario: Entry fits the record budget
- **WHEN** a Shell or Pager log entry serializes to at most 65,536 bytes including LF
- **THEN** the writer appends that complete encoded entry unchanged.

#### Scenario: Entry exceeds the record budget
- **WHEN** a Shell or Pager log entry would exceed 65,536 bytes including LF
- **THEN** the writer appends one valid diagnostic JSONL record identifying `record_omitted` and the 65,536-byte limit, with no bytes from the rejected encoding.

#### Scenario: One oversized record has no line boundary
- **WHEN** an entry contains a message or context too large for the record budget
- **THEN** serialization memory remains bounded by the per-record budget and the persisted file receives the bounded diagnostic line with its terminating LF.
