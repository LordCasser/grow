## ADDED Requirements

### Requirement: Independent Windows session loads coexist with live writers
Independent Windows session loads SHALL use contained observation handles that coexist with the live writer's publication capability. Observation handles SHALL preserve identity and no-reparse validation and SHALL NOT enter the adapter's writer capability cache. Sideband crash reconciliation SHALL remain exclusive to an admitted replacement writer.

#### Scenario: Observe active sideband then replace its writer
- **WHEN** an independent adapter loads a session with a running Sideband and current projections while its writer is alive
- **THEN** ordinary full and light loads succeed without modifying the Sideband, replacement-writer admission remains rejected, and only after the old writer exits can a new writer append one recovery terminal.

#### Scenario: Loaded entity retains its authority
- **WHEN** a session load includes Workflow state or an adapter already has a pinned writer capability
- **THEN** dependent reads reuse the validated entity and do not reopen it through an incompatible or redirected ambient path.

### Requirement: Windows session storage preserves long-path publication and scan exclusions
Windows contained storage SHALL publish immutable artifacts and session entities under valid paths exceeding MAX_PATH while retaining source identity and no-replace semantics. A regular file encountered where a session or cwd directory is expected SHALL be excluded as an invalid entity; operational I/O failures SHALL still fail the scan.

#### Scenario: Publish and restore input artifacts beneath a long profile
- **WHEN** a text/image input payload is stored under a session path exceeding 260 UTF-16 units and then read or stored again
- **THEN** the complete payload round-trips, its reference remains stable, and conflicting pre-existing bytes are preserved and rejected.

#### Scenario: Scan contains ordinary files and valid sessions
- **WHEN** session enumeration encounters ordinary files at cwd and session directory levels alongside a valid session
- **THEN** it skips those files and invalid summaries while returning the valid session.

#### Scenario: Publication target has any legal filename alignment
- **WHEN** a file or session directory is published with a legal name of any UTF-16 alignment
- **THEN** the exact intended name is committed, no extra suffix appears, and existing targets remain unchanged on collision.

#### Scenario: Coordination reads a live source's durable inquiry history
- **WHEN** coordination resolves its source Timeline by session id while that session's publication handle remains live
- **THEN** the read uses the existing independent observation capability, validates the same identity and Timeline, and does not acquire or cache writer authority.

#### Scenario: List sessions while a publication handle is alive
- **WHEN** an independent adapter enumerates summaries while a session writer retains its publication handle
- **THEN** the live session remains visible through observation handles, which do not enter the writer cache; later mutations still acquire their normal writer capability and lease.
