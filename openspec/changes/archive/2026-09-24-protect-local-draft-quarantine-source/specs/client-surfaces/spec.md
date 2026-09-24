## MODIFIED Requirements

### Requirement: Local draft recovery bounds source reads
Local draft recovery SHALL accept only regular file sources, read at most 262,145 bytes, and reject actual content above 262,144 bytes before JSON parsing. Oversized regular files SHALL follow the existing quarantine policy. Immediately before quarantine rename, recovery SHALL verify that the active path still resolves to the source entity opened for validation. If that check observes a replacement, recovery SHALL leave the replacement at the active path and SHALL NOT move or remove it as quarantine for the opened source.
Quarantine publication SHALL fail without replacing an existing quarantine entry.

#### Scenario: File grows after metadata observation
- **WHEN** a draft grows beyond the byte allowance after opening and metadata inspection
- **THEN** bounded reading detects the extra byte and quarantines the draft without restoring its content

#### Scenario: Special file source
- **WHEN** the draft path opens a non-regular file, including a Unix FIFO without a writer
- **THEN** loading returns an error without waiting for a FIFO writer or moving the special file into quarantine

#### Scenario: Regular file at allowance
- **WHEN** valid draft JSON including trailing whitespace exactly fits the allowance, including through a regular-file symlink
- **THEN** recovery preserves the existing record validation and restoration behavior

#### Scenario: Draft path is replaced before quarantine identity check
- **WHEN** recovery reads an invalid or oversized draft from an opened file and the active path is replaced with a different valid draft before the final quarantine identity check
- **THEN** the replacement remains at the active path, is not moved into quarantine, and is not removed

#### Scenario: Quarantine name collides
- **WHEN** a generated quarantine name already belongs to an existing entry
- **THEN** recovery preserves that entry and retries with another name
