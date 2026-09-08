## ADDED Requirements

### Requirement: Debug pruning respects live cooperating writers
Debug file writers SHALL hold shared advisory locks for their file lifetime. Age-based pruning of ordinary log files SHALL acquire a nonblocking exclusive lock and recheck opened-file age before removal; unavailable locks SHALL cause the file to be spared.

#### Scenario: Old idle writer
- **WHEN** an ordinary log is older than retention but a cooperating writer still owns its shared lock
- **THEN** pruning from another process preserves the log path.

#### Scenario: Writer retired
- **WHEN** all writer handles close and the ordinary file is still older than retention
- **THEN** pruning may acquire exclusive ownership and remove it.
