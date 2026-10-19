## ADDED Requirements

### Requirement: Transcript tree export syncs files portably

CLI transcript tree export and trajectory archive preparation SHALL sync each completed transcript file before reporting successful preparation. CLI tree publication SHALL sync the target parent after the no-replace rename. Directory-entry syncing SHALL be attempted only on platforms where the standard filesystem API supports opening directories; errors from attempted directory syncs SHALL fail the operation. Unsupported directory opening on other platforms SHALL NOT prevent export. File creation, write, sync, archive creation, and publication errors SHALL remain failures and SHALL NOT be treated as successful output.

#### Scenario: Windows prepares a transcript tree
- **WHEN** a valid parent and child transcript tree is generated on Windows
- **THEN** each transcript file is fully written and synced, directory traversal does not attempt to open a directory as a regular file, and the export or archive can complete.

#### Scenario: Unix directory sync fails
- **WHEN** syncing a transcript directory on a platform that supports directory handles returns an error
- **THEN** tree preparation fails and the caller does not publish or return a successful archive.

#### Scenario: Transcript file sync fails
- **WHEN** a transcript file cannot be written or synced on any platform
- **THEN** tree preparation fails and the incomplete output is not published or returned as a successful archive.

#### Scenario: Publication parent sync fails after rename
- **WHEN** the no-replace rename succeeds but syncing its parent directory fails
- **THEN** CLI export returns an error without success feedback; the already published complete tree may remain at the target and is not removed.

#### Scenario: HTTP archive response completes or is abandoned
- **WHEN** the trajectory download response reaches end-of-stream or its client drops the response before consuming the full archive
- **THEN** the handler releases the temporary archive in either case, while a fully consumed response contains the complete valid archive.
