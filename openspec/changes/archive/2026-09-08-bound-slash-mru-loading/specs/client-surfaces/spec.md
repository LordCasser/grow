## ADDED Requirements

### Requirement: Slash MRU loading bounds encoded input
Slash MRU loading SHALL accept only regular files with at most 1,048,576 encoded bytes, check metadata on the opened handle, and read at most the allowance plus one byte. Rejected reads SHALL complete initialization with persistence disabled for that store, without overwriting the source.

#### Scenario: File exceeds allowance or grows during reading
- **WHEN** the opened file is too large or a bounded read observes more than the allowance
- **THEN** loading rejects the bytes before JSON parsing and subsequent command use does not produce a persistence snapshot

#### Scenario: Special file on Unix
- **WHEN** the MRU path points to a FIFO without a writer or another non-regular file
- **THEN** loading rejects it without waiting for a FIFO writer and disables persistence

#### Scenario: Normal and missing stores
- **WHEN** a regular file, including a symbolic-link target, contains valid JSON within the allowance
- **THEN** its entries load with the existing 256-entry retention policy
- **WHEN** the path is absent
- **THEN** an empty initialized store remains eligible for persistence
