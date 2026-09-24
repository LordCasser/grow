## ADDED Requirements

### Requirement: Leader candidate retention has a finite spool budget

The leader SHALL retain retractable candidate payloads for non-retracting observers in one transient spool per session. The spool SHALL keep no more than 8 MiB in its in-process buffer before spilling to an unlinked temporary file, and SHALL accept no more than 512 MiB of length-prefixed serialized payloads or 1,000,000 records for one candidate. An accepted candidate SHALL be forwarded in order without materializing the full spool again. A discarded or superseded candidate SHALL release the spool without delivery. A spool budget or I/O failure SHALL close the leader's delivery path without exposing provisional payloads; durable session replay SHALL remain the recovery authority.

#### Scenario: Long candidate crosses memory threshold

- **WHEN** a retractable candidate's serialized records exceed 8 MiB but remain below the disk and record limits
- **THEN** the leader spills them to a temporary file and delivers them in original order only after Accepted.

#### Scenario: Candidate is discarded after spill

- **WHEN** an attempt is discarded after its provisional records have spilled
- **THEN** the spool is released and a non-retracting observer receives none of those records.

#### Scenario: Candidate exceeds spool budget

- **WHEN** a record would exceed the candidate byte or count ceiling, or spool I/O fails
- **THEN** the leader stops the delivery path without forwarding any buffered provisional record to a non-retracting observer.
