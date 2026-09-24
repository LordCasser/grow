## ADDED Requirements

### Requirement: Unified log appends enforce the shared file limit
Each Grow unified-log append SHALL check the live opened inode's length while holding the same exclusive advisory lock as in-place trimming. If the complete append would exceed 5 MiB, the writer SHALL retain only the most recent complete JSONL lines from the existing bounded trim window before appending. The writer SHALL refuse an append that still cannot fit or whose inode cannot be safely trimmed. A completed Grow append SHALL NOT leave a valid shared log larger than 5 MiB; this guarantee assumes other appenders honor the same advisory lock.

#### Scenario: Several writers cross the capacity threshold
- **WHEN** independent Grow writers append enough complete records to cross 5 MiB between maintenance ticks
- **THEN** each completed append leaves the shared inode within 5 MiB and the retained tail consists of complete lines followed by the new record.

#### Scenario: Existing tail cannot be trimmed safely
- **WHEN** the append would exceed 5 MiB and the bounded trim window has no complete line boundary, or on Unix the path no longer names the locked inode
- **THEN** the append is rejected without increasing that inode's length.
