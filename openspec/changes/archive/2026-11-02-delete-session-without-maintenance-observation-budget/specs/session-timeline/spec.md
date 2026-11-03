## ADDED Requirements

### Requirement: Explicit deletion validates canonical references without observation quotas

An explicit single-session deletion SHALL validate the committed journal's existing per-record integrity, sequence, canonical causal state, presentation bindings and receipt ownership under the writer lease. It SHALL determine cross-session and runtime eligibility from those facts without inheriting aggregate read-only/TTL observation or discovery quotas. Provider bodies and presentation/execution artifacts SHALL NOT be required to be readable merely to delete an otherwise eligible session. Automatic maintenance, model recovery and read-only observations SHALL retain their own evidence validation and budgets. Invalid committed canonical evidence SHALL still refuse deletion; quarantine identity checks and cross-session protections SHALL remain.

#### Scenario: Valid long history exceeds observation budget
- **WHEN** an eligible explicit target has a valid journal or sampling evidence references exceeding the maintenance aggregate budget
- **THEN** explicit deletion can complete while bounded observation continues to report budget exhaustion.

#### Scenario: Journal integrity cannot be established
- **WHEN** committed journal checksum, sequence, canonical causality or receipt ownership is invalid
- **THEN** deletion is refused and the target directory remains.
