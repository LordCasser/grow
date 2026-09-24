## MODIFIED Requirements

### Requirement: Search-replace creation requires confirmed absence

When `search_replace` receives an empty old string, it SHALL treat a failed read as evidence of absence only if the filesystem explicitly reports NotFound. Other read failures SHALL stop before any write or file-written notification. A confirmed missing target SHALL be created only through a filesystem operation that atomically refuses to replace an existing target; if that operation is unsupported or another writer creates the target first, the tool SHALL report failure without a file-written notification. Existing empty-file updates remain allowed; this requirement does not claim cross-writer conflict protection for those updates or ordinary replacement.

#### Scenario: Existing file cannot be read
- **WHEN** the target read fails with PermissionDenied or an error without a preserved NotFound kind
- **THEN** the tool returns a failure that identifies the read error, without attempting a write or publishing a file-written notification.

#### Scenario: Target is absent
- **WHEN** the target read explicitly reports NotFound and the filesystem supports exclusive creation
- **THEN** the full new content is committed to the target without exposing a partial result, and a file-written notification is published after success.

#### Scenario: Another writer creates the target after the read
- **WHEN** the target read explicitly reports NotFound but a competing process creates the final path before the exclusive commit
- **THEN** `search_replace` reports a conflict without replacing the competitor's bytes or publishing a file-written notification.

#### Scenario: Filesystem cannot create exclusively
- **WHEN** the target read explicitly reports NotFound but its filesystem lacks a no-replace creation operation
- **THEN** `search_replace` reports that limitation before any ordinary write or file-written notification.

#### Scenario: Parent component is a file
- **WHEN** the target read reports that a path component is not a directory
- **THEN** the tool reports that read failure before attempting to create the target.

#### Scenario: Existing file is empty
- **WHEN** the target read succeeds with an empty byte sequence
- **THEN** the ordinary empty-file update path remains available.
