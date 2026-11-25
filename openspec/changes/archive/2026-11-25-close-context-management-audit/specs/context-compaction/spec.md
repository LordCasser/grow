## ADDED Requirements

### Requirement: Preventive summaries make progress and preserve primary admission
Preventive compaction SHALL summarize a range only if it includes replaceable evidence beyond already committed summaries. A settled provider failure during uncommitted preventive summary generation SHALL yield to normal primary admission when the primary request fits including its output reservation and the route and account permit sampling. Provider-overflow recovery, hard overflow, cancellation, budget limits and persistence failures SHALL retain their existing authority.

#### Scenario: A protected tail remains large across tool steps
- **WHEN** a new step adds a tool result but the only eligible source consists of a previous summary
- **THEN** no second preventive summary is dispatched for that range. A later eligible range containing the old summary and new evidence can be summarized.

#### Scenario: The preventive provider fails
- **WHEN** a preventive summary sampling attempt fails without a replacement and the complete primary request passes ordinary admission
- **THEN** the failed attempt remains recorded and ordinary sampling can proceed without repeatedly attempting that same failed maintenance in the turn.

#### Scenario: A required recovery or durable operation fails
- **WHEN** the primary request cannot pass admission, a provider overflow requires recovery, cancellation occurs, or settlement or storage fails
- **THEN** a failed summary does not grant primary dispatch or acknowledge a successful replacement.

#### Scenario: Only the preventive summary exceeds its provider window
- **WHEN** the summary provider rejects the source as too large and the bounded reduction cannot produce a usable summary, while the primary request independently passes admission
- **THEN** the recorded preventive failure may yield to that primary request. A hard overflow on the primary request still requires recovery.

#### Scenario: Cancellation overlaps a failed durable acknowledgement
- **WHEN** a promoted background summary is cancelled while its worker is settling and the durable acknowledgement fails
- **THEN** the fatal persistence error remains visible, the worker is joined, and no primary request is dispatched.
