## ADDED Requirements

### Requirement: Crash evidence survives failed report publication

Crash recovery SHALL return an observable error for existing-file read or report/history publication failure. It SHALL retain valid raw evidence until both its primary report and history report have been published with supported file and directory durability barriers. Published reports SHALL be private on Unix from creation, and failure SHALL NOT return a nonexistent report path as success. A partially published report SHALL NOT cause raw evidence to be consumed before the remaining publication succeeds. Missing or empty raw files SHALL remain a normal no-crash observation; nonempty malformed evidence SHALL remain available with an explicit parse error.

#### Scenario: Primary report cannot be published

- **WHEN** a valid raw crash exists but primary report publication fails
- **THEN** recovery returns an error, the original raw bytes remain available, and no report success is claimed.

#### Scenario: History publication fails after primary success

- **WHEN** primary publication succeeds but history creation, write or supported synchronization fails
- **THEN** recovery reports failure and retains the same raw crash for retry; the published primary report remains available.

#### Scenario: Storage recovers

- **WHEN** the same preserved crash is retried after publication storage recovers
- **THEN** recovery reports its original identity only after primary and history are committed, and raw consumption follows that commit.

#### Scenario: Older crash is recovered after newer history

- **WHEN** existing history is at its retention bound and a recovered crash has an older timestamp than the retained records
- **THEN** that call's pruning preserves the just-published crash archive, prunes other oldest eligible records within the existing bound, and only then permits raw consumption.

#### Scenario: Startup recovery is blocked

- **WHEN** CLI startup receives a crash recovery error
- **THEN** it reports the blocked recovery and retains terminal restoration without installing capture that truncates the preserved raw file.
