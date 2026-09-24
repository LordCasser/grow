## ADDED Requirements

### Requirement: Child transcript file notices retain the export origin

An explicit `/export` or `/copy` file job started from a child Agent view SHALL snapshot that child's content, cwd, Agent identity and session identity. Admission and completion notices SHALL appear in that same child view even if the user switches to the parent before the job completes. A removed or rebound child SHALL NOT receive the old job's feedback; the file queue SHALL still advance.

#### Scenario: Minimal child export completes after a view switch

- **WHEN** `/export` is submitted from a Minimal child view whose content and cwd differ from its parent, then the user switches to the parent before the write completes
- **THEN** the file contains the child's transcript at the child's resolved path, the child receives saving and completion notices, and the parent receives neither notice

#### Scenario: Child is removed or rebound before completion

- **WHEN** the child view that submitted a file job is removed or bound to another session before the result arrives
- **THEN** the file job settles and the queue advances without publishing the old completion to a different view or session
