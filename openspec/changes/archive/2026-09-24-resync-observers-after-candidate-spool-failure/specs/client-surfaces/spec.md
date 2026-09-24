## MODIFIED Requirements

### Requirement: Leader candidate retention has a finite spool budget

The leader SHALL retain retractable candidate payloads for non-retracting observers in one transient spool per session. The spool SHALL keep no more than 8 MiB in its in-process buffer before spilling to an unlinked temporary file, and SHALL accept no more than 512 MiB of length-prefixed serialized payloads or 1,000,000 records for one candidate. An accepted candidate SHALL be forwarded in order without materializing the full spool again. A discarded or superseded candidate SHALL release the spool without delivery. A spool budget or I/O failure SHALL discard that candidate's transient spool while preserving subscriptions and independent notification delivery. If the candidate is Accepted, the leader SHALL request an in-place full canonical reload for non-retracting observers after any in-flight load; Grow Pager SHALL perform that reload for its attached root, including when the failed candidate belongs to a child session. If Discarded, the leader SHALL NOT request resync. No unaccepted candidate payload SHALL reach those observers.

#### Scenario: Long candidate crosses memory threshold

- **WHEN** a retractable candidate's serialized records exceed 8 MiB but remain below the disk and record limits
- **THEN** the leader spills them to a temporary file and delivers them in original order only after Accepted.

#### Scenario: Candidate is discarded after spill

- **WHEN** an attempt is discarded after its provisional records have spilled
- **THEN** the spool is released and a non-retracting observer receives none of those records.

#### Scenario: Candidate exceeds spool budget

- **WHEN** a candidate record exceeds the byte or count ceiling before Accepted
- **THEN** the leader releases its spool, keeps its clients and session ownership, and withholds all provisional records from non-retracting observers.

#### Scenario: Failed candidate is accepted or discarded

- **WHEN** a failed candidate reaches Accepted, including after a spool read fails mid-flush
- **THEN** the leader signals each affected non-retracting observer after any in-flight load; Grow Pager reloads canonical history in place, replacing any partial presentation.
- **WHEN** a failed candidate reaches Discarded
- **THEN** those observers receive no candidate record and require no resync.

#### Scenario: Observer attaches after failure

- **WHEN** an observer loads the session after the failed candidate has reached a terminal boundary
- **THEN** its normal durable load supplies accepted history without transient backfill.
