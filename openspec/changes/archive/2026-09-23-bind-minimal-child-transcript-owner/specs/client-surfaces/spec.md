## ADDED Requirements

### Requirement: Minimal transcripts retain the selected view owner

Minimal `/transcript` SHALL capture the selected root or child Agent view and session identity when invoked. Its incremental rendering, cwd/media resolution, external pager handoff and failure notices SHALL use that captured view across focus changes. A removed or rebound child SHALL NOT be replaced by a parent or another child with matching entry IDs.

#### Scenario: Child transcript survives a focus switch

- **WHEN** `/transcript` begins in a Minimal child view with content and cwd distinct from the parent, and focus returns to the parent during the build
- **THEN** every slice and the pager file use the child content and cwd; pager failure feedback targets the same child

#### Scenario: Captured child is removed or rebound

- **WHEN** the child view disappears or binds to a different session before the build finishes
- **THEN** the in-flight build is dropped without opening a pager or publishing feedback to another conversation

#### Scenario: Owner reload during a build

- **WHEN** the captured view enters session reload while its transcript is still being rendered
- **THEN** the old prefix is discarded and the build waits for that view's final state before restarting
