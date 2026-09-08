## ADDED Requirements

### Requirement: Unified log trimming reads a bounded tail
Unified log trimming SHALL read at most MAX_SIZE / 2 bytes (2.5 MiB) from the later of the opened file midpoint and its final MAX_SIZE / 2 bytes. It SHALL retain only bytes after the first newline in that window, preserve the inode, and leave the file unchanged if no newline exists there.

#### Scenario: Oversized log
- **WHEN** a log is much larger than MAX_SIZE and has complete lines in its trailing window
- **THEN** trimming reads no more than MAX_SIZE / 2 bytes and retains recent complete lines within that budget.

#### Scenario: No line boundary
- **WHEN** the admitted trailing window contains no newline
- **THEN** trimming leaves the source unchanged.
