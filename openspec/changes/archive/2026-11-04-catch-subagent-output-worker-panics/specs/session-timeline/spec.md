## ADDED Requirements

### Requirement: Persisted output reader panics are isolated to their observation
When the build enables panic unwinding, the subagent coordinator SHALL isolate an unwind from one persisted-output reader to that deduplicated read. Every query or inspection waiting on that read SHALL receive `Unavailable`; the reader's owned permit SHALL be released as its future unwinds; and the coordinator SHALL continue servicing unrelated requests. The panic boundary SHALL NOT include child lifecycle, validation, or other coordinator owners, and the failed read SHALL NOT be retried automatically.

#### Scenario: A persisted-output reader panics
- **WHEN** a query or inspection awaits persisted output and the reader future panics with unwinding enabled
- **THEN** all targets attached to that read receive `Unavailable`, a later request can use the released reader slot, and the coordinator remains available for unrelated lifecycle and observation messages.

#### Scenario: Build uses panic abort
- **WHEN** the selected build profile uses panic=abort
- **THEN** the existing process-abort policy remains authoritative; this observation boundary does not promise recovery from an abort.
