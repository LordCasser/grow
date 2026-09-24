## ADDED Requirements

### Requirement: Local draft quarantine retention is bounded
Local draft quarantine SHALL retain at most 64 regular-file or symbolic-link entries and at most 16 MiB in total. When either limit is exceeded, it SHALL delete the oldest quarantined entries first; entries with equal or unavailable modification times SHALL be ordered by filename. Symlinks SHALL be measured by their own metadata and SHALL not be followed. Other non-regular entries SHALL not be counted.

#### Scenario: Quarantine exceeds the file count
- **WHEN** a local draft is quarantined while the quarantine directory contains more than 64 regular-file or symbolic-link entries
- **THEN** the oldest files are removed until at most 64 remain, with filename ordering deciding equal-time entries

#### Scenario: Quarantine exceeds the byte budget
- **WHEN** a local draft is quarantined and regular-file plus symbolic-link metadata bytes exceed 16 MiB
- **THEN** oldest entries are removed until retained bytes are at most 16 MiB, even if the remaining entry count is below 64

#### Scenario: Quarantine timestamps are unavailable or tied
- **WHEN** multiple quarantine entries have equal or unavailable modification times
- **THEN** reclamation order is resolved by filename and is independent of directory enumeration order

#### Scenario: Symlink and other special quarantine entries
- **WHEN** the quarantine directory contains a symlink or another special entry
- **THEN** reclamation counts a symlink using its own metadata without following it, ignores other special entries, and continues to apply both limits
