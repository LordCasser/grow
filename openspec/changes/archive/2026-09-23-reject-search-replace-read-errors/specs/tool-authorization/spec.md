## ADDED Requirements

### Requirement: Search-replace creation requires confirmed absence

When `search_replace` receives an empty old string, it SHALL treat a failed read as evidence of absence only if the filesystem explicitly reports NotFound. Other read failures SHALL stop before any write or file-written notification. Existing empty-file updates and confirmed missing-file creation remain allowed; this requirement does not claim atomic create or cross-writer conflict protection.

#### Scenario: Existing file cannot be read
- **WHEN** the target read fails with PermissionDenied or an error without a preserved NotFound kind
- **THEN** the tool returns a failure that identifies the read error, without attempting a write or publishing a file-written notification.

#### Scenario: Target is absent
- **WHEN** the target read explicitly reports NotFound
- **THEN** the ordinary new-file write path remains available, subject to its existing write-error handling.

#### Scenario: Parent component is a file
- **WHEN** the target read reports that a path component is not a directory
- **THEN** the tool reports that read failure before attempting to create the target.

#### Scenario: Existing file is empty
- **WHEN** the target read succeeds with an empty byte sequence
- **THEN** the ordinary empty-file update path remains available.
