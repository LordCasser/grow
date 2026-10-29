## ADDED Requirements
### Requirement: Turn termination clears foreground compaction feedback
Pager SHALL clear foreground compaction live feedback when its turn terminates, in addition to normal compaction terminal handling. Unrelated feedback SHALL remain governed by its own lifecycle.

#### Scenario: Turn fails after compaction started
- **WHEN** a turn terminates while foreground compaction feedback is still displayed
- **THEN** the session is idle without stale Compacting feedback and later terminal notifications do not reopen the activity.
