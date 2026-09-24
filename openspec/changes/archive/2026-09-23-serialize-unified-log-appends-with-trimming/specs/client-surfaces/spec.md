## ADDED Requirements

### Requirement: Unified log appends coordinate with in-place trimming
Unified log writers SHALL acquire the opened log inode's exclusive advisory lock before appending a complete encoded entry, using the same lock that guards in-place trim rewrite and truncate. A trim encountering an active append SHALL yield under its existing nonblocking lock policy. An append encountering an active trim SHALL wait until that trim releases the lock before writing, so a successfully written line is not removed by that trim's truncation.

#### Scenario: Trim owns the inode while a writer appends
- **WHEN** trimming has locked the log inode and a writer starts an append before trimming rewrites and truncates it
- **THEN** the append completes after the trim releases the lock and remains after the retained tail

#### Scenario: Append owns the inode while trim is attempted
- **WHEN** a writer holds the inode lock for an append and another process attempts a nonblocking trim
- **THEN** that trim leaves the inode unchanged and may be retried on later maintenance
