## ADDED Requirements

### Requirement: Memory daily-log writes report actual persistence

The memory storage API SHALL distinguish a daily log that was written from one skipped by the existing ephemeral-workspace policy. A path SHALL be returned only for a write that completed successfully. Memory flush and session-end handling SHALL NOT report an ephemeral skip as written; a skipped flush SHALL NOT advance its successful history checkpoint.

#### Scenario: Daily log write is suppressed for an ephemeral workspace

- **WHEN** `write_daily_log` is called for an ephemeral workspace
- **THEN** it returns an explicit skipped result, does not create the sessions directory or file, and callers do not present the expected path as a persisted log

#### Scenario: Daily log write completes

- **WHEN** `write_daily_log` successfully writes a daily log for a persistent workspace
- **THEN** it returns the written path, and memory flush may reindex the file and report `written`

#### Scenario: Memory flush is skipped by ephemeral policy

- **WHEN** an accepted flush summary reaches `write_daily_log` for an ephemeral workspace
- **THEN** completion and diagnostics report skipped rather than error, with no path, no reindex or persisted-write marker, and the history remains eligible for a later retry

#### Scenario: Session-end summary is skipped by ephemeral policy

- **WHEN** an eligible session-end summary reaches `write_daily_log` for an ephemeral workspace
- **THEN** the hook reports skipped rather than `Written`
