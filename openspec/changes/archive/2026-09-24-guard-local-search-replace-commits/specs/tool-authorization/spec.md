## ADDED Requirements

### Requirement: Local search-replace commits compare the source bytes

For an existing target, `search_replace` SHALL commit an edit only when its filesystem adapter can conditionally write the exact bytes read for that edit. The local adapter SHALL serialize cooperating writers across processes on the opened file with a bounded lock acquisition, and reject a mismatch or replaced target before reporting success. An adapter without this operation SHALL fail the edit. A failed conditional commit SHALL not emit a file-written notification. This contract does not assert atomicity against an external writer that ignores advisory locks.

#### Scenario: Two Grow edits read the same version
- **WHEN** two Grow writers read the same existing file and propose different replacements
- **THEN** at most one commits against that version; the other reports a conflict without publishing a file-written notification.

#### Scenario: Existing empty file changes before commit
- **WHEN** an empty target is read for `old_string = ""` and another writer changes it before the conditional commit
- **THEN** the creation-style edit reports a conflict and preserves the newer bytes.

#### Scenario: Path no longer names the opened source
- **WHEN** a target path is replaced after the local adapter opens its source file
- **THEN** the conditional edit refuses to report a successful write to the replacement path.

#### Scenario: Adapter lacks conditional write
- **WHEN** an existing-file edit is routed through a filesystem adapter without conditional write support
- **THEN** the edit fails without an unconditional fallback or file-written notification.

#### Scenario: Target lock remains held
- **WHEN** a competing process holds the target lock past the local acquisition deadline
- **THEN** the edit fails without writing or publishing a file-written notification.
