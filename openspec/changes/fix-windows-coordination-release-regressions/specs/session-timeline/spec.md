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
