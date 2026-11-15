## MODIFIED Requirements

### Requirement: Workflow restore retains the latest valid runs
Workflow writer cold recovery SHALL restore only Workflow runs whose Timeline lifecycle is open or whose latest boundary marks execution suspended. It SHALL NOT scan closed Workflow history or open historical run sidecars merely to initialize execution readiness. Historical Workflow operations SHALL retain the existing complete-history restore path when explicitly requested. Execution-required runs SHALL retain directory capability checks, per-file read bounds, lifecycle validation, Timeline-seed repair, and existing fatal error behavior.

#### Scenario: Closed Workflow history does not delay writer readiness
- **WHEN** a session has fixed execution state and an increasing number of closed Workflow runs
- **THEN** writer cold recovery opens no historical Workflow sidecars and restores the execution-required set without scanning closed candidates

#### Scenario: Active and suspended Workflow runs restore for execution
- **WHEN** Timeline contains open runs or runs whose latest Ended boundary is execution-suspended
- **THEN** writer cold recovery restores and validates each such run; an absent sidecar uses the Timeline seed and follows the existing queued repair behavior

#### Scenario: More valid runs than capacity
- **WHEN** an explicit full-history restore sees more than 128 valid runs
- **THEN** it returns the latest 128 in Timeline order; this capacity does not restrict execution-required cold recovery

#### Scenario: Recent candidates are unusable
- **WHEN** an explicit full-history restore skips newer cleared, missing, or invalid candidates while older candidates remain valid
- **THEN** it continues until it returns 128 valid runs or exhausts history; cold execution recovery considers only its required run IDs

#### Scenario: Corrupt sidecar is repaired before writer startup completes
- **WHEN** execution-required cold recovery uses the Timeline seed for a corrupt Workflow sidecar whose bytes were fingerprinted
- **THEN** it persists and acknowledges the repair before actor initialization completes; an explicit full-history restore retains the same repair behavior for runs it visits

#### Scenario: Sidecar changes during repair
- **WHEN** a visited run's sidecar changes after it was read as corrupt but before the repair lock is acquired
- **THEN** repair reports an observable error and neither overwrites the new contents nor reports successful actor initialization

#### Scenario: Historical Workflow management is requested
- **WHEN** a caller requests the existing full Workflow history view
- **THEN** it uses the complete-history restore path and retains the existing latest-valid-runs ordering, capacity, and validation behavior

证据：`crates/codegen/chat-state/src/timeline.rs` — Workflow lifecycle projection；`crates/codegen/shell/src/session/storage/jsonl/mod.rs` — writer and full-history restore paths。
