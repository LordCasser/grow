## ADDED Requirements

### Requirement: Explicit permanent deletion can abandon independent local continuation

An explicit permanent deletion SHALL be allowed to abandon an independent session's unconsumed local suspension or restoration continuation after the existing writer ownership, canonical cross-session reference and quarantine identity checks succeed. The quarantine commit SHALL dispose of that local history; deletion SHALL NOT synthesize ExecutionReleased, resume work, claim completion, or recursively delete another session. An active competing writer or a canonical cross-session dependency SHALL still refuse deletion. Automatic maintenance SHALL continue preserving unconsumed runtime handoffs, and recovery/import/rewind SHALL retain their existing evidence rules.

#### Scenario: User permanently deletes an independent suspended history
- **WHEN** explicit deletion targets independent Suspending, Suspended or Restoring history without a competing writer
- **THEN** quarantine and permanent removal can complete without first restoring it or appending a fabricated execution release.

#### Scenario: Automatic cleanup sees the same continuation
- **WHEN** TTL maintenance sees the same unconsumed runtime history
- **THEN** it retains the history and its recovery responsibility.

#### Scenario: A writer still owns the target
- **WHEN** another writer holds the independent target while explicit deletion is requested
- **THEN** deletion is refused, the bytes remain unchanged, and the user can retry after writer settlement.
