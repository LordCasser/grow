## ADDED Requirements

### Requirement: Unified log disk writes do not block producers
Shell and Pager unified-log producers SHALL enqueue complete bounded records without waiting for a filesystem append, an inode lock, or trim. The process-local queue SHALL have a fixed record count and record byte bound. Overflow SHALL be observable as a coalesced diagnostic loss count; it SHALL NOT block a caller indefinitely or silently claim delivery. Snapshot and normal shutdown SHALL use a bounded flush wait and SHALL NOT claim to cancel an OS write already in progress.

#### Scenario: Filesystem append remains blocked
- **WHEN** the unified-log worker is held inside a slow append or inode lock
- **THEN** producer calls return after enqueue or bounded-queue overflow without waiting for that write.

#### Scenario: Queue saturates and later drains
- **WHEN** the worker falls behind enough to fill the queue and later resumes
- **THEN** dropped records are counted and a complete diagnostic record reports the coalesced loss before subsequent accepted records.

#### Scenario: Snapshot or shutdown meets a blocked worker
- **WHEN** a flush barrier cannot enter or cross the queue within its deadline
- **THEN** the caller stops waiting and does not claim all queued records were written.
