## MODIFIED Requirements

### Requirement: Independent Windows session loads coexist with live writers

Independent Windows session loads, including child history streams and replay preparation, SHALL use contained observation handles that coexist with the live writer's publication capability. Observation handles SHALL preserve identity and no-reparse validation and SHALL NOT enter the adapter's writer capability cache. Sideband crash reconciliation SHALL remain exclusive to an admitted replacement writer. Reading history SHALL NOT restore execution, acquire writer ownership or mutate canonical history.

#### Scenario: Observe active sideband then replace its writer
- **WHEN** an independent adapter loads a session with a running Sideband and current projections while its writer is alive
- **THEN** ordinary full and light loads succeed without modifying the Sideband, replacement-writer admission remains rejected, and only after the old writer exits can a new writer append one recovery terminal.

#### Scenario: Loaded entity retains its authority
- **WHEN** a session load includes Workflow state or an adapter already has a pinned writer capability
- **THEN** dependent reads reuse the validated entity and do not reopen it through an incompatible or redirected ambient path.

#### Scenario: Child history is opened while publication capability remains live
- **WHEN** a Windows child view prepares or streams its persisted history while the exact session entity has a retained DELETE-capable publication handle
- **THEN** public history/replay reads use existing contained shared observation, validate and emit the same ordered canonical history without a sharing violation or history mutation, and do not gain writer or execution authority.

#### Scenario: History source is invalid
- **WHEN** a history read resolves a reparse point, inconsistent identity, duplicate canonical id or corrupt committed history
- **THEN** it retains existing failure semantics rather than bypassing containment or returning partial success to avoid a sharing error.
