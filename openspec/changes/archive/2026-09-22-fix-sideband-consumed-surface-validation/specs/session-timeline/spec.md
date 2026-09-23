## ADDED Requirements

### Requirement: Sideband coordinates follow canonical Timeline Surface producers

Sideband parent validation SHALL accept a historical Surface coordinate exactly when the referenced parent Timeline event canonically produced that item, including consumed user input and consumed notification input. It SHALL continue to reject non-Surface lifecycle facts, wrong event identity, out-of-range items and coordinates outside the frozen source/input ranges. Strict reload SHALL apply the same rule as live Timeline materialization.

#### Scenario: Reload a compaction selected from consumed inputs

- **WHEN** a completed compaction Sideband selected valid coordinates produced by `Input::Consumed` or `Notification::Consumed` with model input
- **THEN** strict session reload accepts the Sideband ledger and reconstructs the same Surface without migration or ledger rewrite.

#### Scenario: Reject a non-Surface coordinate

- **WHEN** a Sideband manifest names an Observation, lifecycle-only Input/Notification event, or an item outside a producing event's cardinality
- **THEN** parent validation rejects the ledger as an invalid Surface selection.
