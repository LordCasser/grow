## ADDED Requirements

### Requirement: Scroll recorder rejects special file targets
The scroll recorder SHALL accept only regular file targets, checking the opened handle before truncation. Unix FIFO opening SHALL not wait for a reader. Rejected targets SHALL use the existing disabled-recorder failure state.

#### Scenario: FIFO target
- **WHEN** an explicit scroll-log target is a FIFO, with or without a reader
- **THEN** first recording fails without waiting for a FIFO peer and disables recording without writing JSONL to that pipe

#### Scenario: Existing regular target
- **WHEN** the target is a regular file or a symbolic link to one
- **THEN** preserve lazy opening and replacement of previous file contents on first record
