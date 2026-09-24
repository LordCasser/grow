## REMOVED Requirements

### Requirement: Debug latest-link updates preserve unowned temporaries

### Requirement: Debug routing retains only selected writer guards

### Requirement: Debug pruning respects live cooperating writers

## ADDED Requirements

### Requirement: Debug firehose is a bounded attributed stream

When enabled, the debug firehose SHALL write complete lines to one selected file with role, process ID and session ID attribution; payload line breaks SHALL be escaped inside their record. Its producer SHALL enqueue without filesystem I/O through one bounded process queue and one disk worker. Each complete line SHALL be at most 65,536 bytes and the queue SHALL hold no more than 64 records. Oversized lines SHALL end with a truncation marker; queue overflow SHALL be observable as a coalesced loss marker when writing resumes. A bounded flush SHALL wait for previously accepted records without claiming to cancel a blocked OS write or guarantee durable sync.

#### Scenario: Concurrent sessions

- **WHEN** several sessions and fallback events produce debug records
- **THEN** the one selected stream contains independently attributable complete lines without creating per-session writer workers or files.

#### Scenario: Producer outpaces disk

- **WHEN** disk writing stalls and the pending queue fills
- **THEN** producers return without waiting for disk and the next successful write reports the number of lost records.

### Requirement: Debug firehose retains a bounded complete-line tail

Each cooperating debug writer SHALL lock the opened inode exclusively, check its live size and keep the file at or below 32 MiB after a completed append. An overflowing append SHALL retain only complete recent lines from a bounded tail before adding the new complete record. The writer SHALL detect path replacement before append and reopen or drop rather than knowingly write to a detached descriptor. Age-based cleanup of legacy per-session files SHALL spare cooperating open writers.

#### Scenario: Shared path reaches its ceiling

- **WHEN** multiple Grow processes append enough complete records to exceed 32 MiB
- **THEN** each completed append preserves complete recent lines and the shared file does not exceed the ceiling.

#### Scenario: Selected path is replaced

- **WHEN** the debug path no longer names the worker's opened inode before its next append
- **THEN** the worker reopens the selected path or drops the record without continuing on the known detached inode.
