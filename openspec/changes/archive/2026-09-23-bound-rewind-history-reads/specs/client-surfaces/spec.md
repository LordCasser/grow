## ADDED Requirements

### Requirement: Pinned rewind history reads have bounded and visible failure
Pinned rewind metadata and full-history reads SHALL reject a nonblank record above 64 MiB, a scan above 256 MiB, or more than 50,000 records before accepting a partial history projection. A pinned read SHALL not block the async request executor while scanning the file. Failed or cancelled scans SHALL retain the pinned source and live points for a later retry.

#### Scenario: Oversized pinned record or scan
- **WHEN** a pinned rewind record exceeds 64 MiB, a scan exceeds 256 MiB, or a scan has more than 50,000 records
- **THEN** metadata and full-history reads fail without merging a valid prefix or treating the file as empty.

#### Scenario: Cancelled read
- **WHEN** a metadata or full-history request is cancelled while its blocking scan remains in progress
- **THEN** another scan cannot seek the same pinned source until the worker releases its read ownership, and the source remains available for retry.

#### Scenario: Damaged nested snapshot
- **WHEN** metadata encounters a record whose nested snapshot shape cannot be loaded as a rewind point
- **THEN** the picker request fails instead of claiming that checkpoint has no file changes.

#### Scenario: Picker failure feedback
- **WHEN** metadata scanning fails for the active pinned source
- **THEN** the points request reports an error to the client; the current rewind interaction closes, restores its draft, and shows failure rather than a conversation-only choice.

## MODIFIED Requirements

### Requirement: Pinned rewind parsing rejects malformed records
Pinned rewind JSONL readers SHALL reject a nonblank record that fails deserialization, returning InvalidData with its one-based physical line number rather than a successful partial record set. Historical load failure SHALL retain the source and live points for retry. Metadata scans SHALL validate nested snapshot types without retaining file content and SHALL report a parse failure to the picker.

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
- **WHEN** the pinned metadata reader encounters a record it cannot deserialize, including a malformed nested snapshot
- **THEN** the picker request reports failure without consuming the historical source or presenting in-memory points as a complete checkpoint list.
