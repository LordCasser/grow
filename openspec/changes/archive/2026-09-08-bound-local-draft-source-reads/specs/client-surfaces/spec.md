## ADDED Requirements

### Requirement: Local draft recovery bounds source reads
Local draft recovery SHALL accept only regular file sources, read at most 262,145 bytes, and reject actual content above 262,144 bytes before JSON parsing. Oversized regular files SHALL follow the existing quarantine policy.

#### Scenario: File grows after metadata observation
- **WHEN** a draft grows beyond the byte allowance after opening and metadata inspection
- **THEN** bounded reading detects the extra byte and quarantines the draft without restoring its content

#### Scenario: Special file source
- **WHEN** the draft path opens a non-regular file, including a Unix FIFO without a writer
- **THEN** loading returns an error without waiting for a FIFO writer or moving the special file into quarantine

#### Scenario: Regular file at allowance
- **WHEN** valid draft JSON including trailing whitespace exactly fits the allowance, including through a regular-file symlink
- **THEN** recovery preserves the existing record validation and restoration behavior
