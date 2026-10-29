## ADDED Requirements
### Requirement: Compaction completion and admission use effective context pressure
Manual and synchronous compaction completion and post-replacement window checks SHALL use the current effective request projection with known request envelope and provider calibration, never a raw Surface delta intermediate. A subsequent full request SHALL refresh its current envelope and pass the independent admission check. Background completion SHALL retain its existing deferred request-projection semantics.

#### Scenario: Retained reasoning after manual replacement
- **WHEN** a manual summary replaces a range while raw reasoning remains in history
- **THEN** completion reports nonempty effective retained content accurately and repeated request assembly does not introduce phantom growth.

### Requirement: Every foreground compaction start has a terminal presentation
Every published foreground or promoted compaction Started SHALL receive one terminal presentation after the transaction settles. A committed replacement SHALL be presented as completed even if a post-commit error fails its enclosing turn; failures before commit and cancellation SHALL use their corresponding terminal. A new committed replacement SHALL invalidate an older pending asynchronous completion notice regardless of the later enclosing result.

#### Scenario: Committed replacement remains over window
- **WHEN** replacement commits and convergence validation fails the turn
- **THEN** Timeline and presentation both settle compaction, the turn error propagates, and the UI does not remain compacting.

#### Scenario: Replacement supersedes deferred completion then fails
- **WHEN** a pending async notice exists and a manual or synchronous replacement commits but later returns an error
- **THEN** the older notice is discarded and cannot attribute the new request size to the old transaction.

#### Scenario: Summary generation is cancelled
- **WHEN** a foreground or promoted summary generation is cancelled before replacement
- **THEN** its foreground owner publishes exactly one Cancelled terminal, without a second terminal from the generation layer.

#### Scenario: Pruning loses persistence acknowledgement
- **WHEN** pre-prune cannot confirm its write or the ChatState owner is unavailable
- **THEN** the foreground compaction settles with failure and does not fall through to a summary provider request; a plan rejected before writing may still use the normal summary path.
