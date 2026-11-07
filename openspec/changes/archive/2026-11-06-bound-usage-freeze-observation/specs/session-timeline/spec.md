## ADDED Requirements

### Requirement: Prompt usage freeze observes within its absolute deadline

Shell prompt-end usage freeze SHALL bound every Outstanding subagent-usage observation, including coordinator queueing and the one-shot ACK wait, by the freeze's single absolute deadline. A receiver that keeps the responder alive without replying SHALL NOT extend the freeze past that deadline; expiry SHALL follow the existing query-failure fail-closed semantics, marking the prompt report and both usage ledgers incomplete without terminating the child, recording zero usage or creating another ledger. Late usage folds SHALL keep settling on the session ledger through the existing attribution path. The turn-error usage fallback SHALL use the same bounded observation convention as cancellation: a short fixed window whose expiry maps to query failure. Zero remaining budget MAY consume an immediately ready observation or fail closed. Cancellation's own bounded query and the existing freeze outcomes (clean, background-only, sticky-only, foreground-live timeout) SHALL remain unchanged.

#### Scenario: ACK is held without replying

- **WHEN** the freeze sends an Outstanding query and the receiver keeps the respond_to channel alive without sending a reply
- **THEN** the freeze ends at its original deadline with the prompt report and both usage ledgers marked incomplete, and the child is neither terminated nor billed as zero.

#### Scenario: Late settlement after the deadline

- **WHEN** a foreground child's usage fold arrives after the freeze failed closed
- **THEN** the settlement still lands on the session ledger through the existing attribution path without duplicate billing.

#### Scenario: Error-path fallback query is bounded

- **WHEN** a turn fails and its completion error path queries outstanding usage while the receiver holds the responder without replying
- **THEN** the fallback ends within its short observation window and reports usage as possibly under-counting instead of blocking completion.

#### Scenario: Existing freeze outcomes unchanged

- **WHEN** the receiver answers promptly with an empty outstanding set, a background-only child, a sticky report-only state, or keeps a foreground child live until the deadline
- **THEN** the freeze retains the existing clean, background report-only, sticky report-only and fail-closed outcomes respectively.
