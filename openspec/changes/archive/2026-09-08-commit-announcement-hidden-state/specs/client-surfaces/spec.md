## ADDED Requirements

### Requirement: Hidden announcement state commits complete snapshots
Hidden announcement state persistence SHALL publish complete canonical snapshots without truncating the current destination during preparation. A failed commit SHALL propagate failure to the pager persistence result and clean up only its own temporary artifacts.

#### Scenario: Snapshot write succeeds
- **WHEN** a hidden-ID snapshot is committed
- **THEN** the destination contains a complete canonical hidden_ids document

#### Scenario: Preparation fails
- **WHEN** writing or preparing the replacement fails before publication
- **THEN** the prior committed destination is preserved and failure is reported
