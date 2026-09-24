## ADDED Requirements

### Requirement: macOS clipboard transfer files have a per-file write budget
macOS clipboard AppleScript subprocesses SHALL inherit a regular-file size limit no greater than 50,000,000 bytes. Format fallback SHALL catch image coercion failures only; once coercion succeeds, transfer-file open/write/close failures SHALL propagate as an image-read error and the owned private temporary directory SHALL be cleaned. This bounds each file, not aggregate temporary storage, helper/AppKit memory, or later image decoding.

#### Scenario: Transfer file exceeds the write budget
- **WHEN** an AppleScript subprocess attempts to write more than 50,000,000 bytes to one transfer file
- **THEN** the write cannot extend that file beyond the limit, the clipboard operation returns an error, and its private temporary directory is removed.

#### Scenario: Transfer file fits the write budget
- **WHEN** an AppleScript subprocess writes at most 50,000,000 bytes to one transfer file
- **THEN** the child file-size limit does not prevent the write and existing clipboard result handling remains available.
