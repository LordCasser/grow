## ADDED Requirements

### Requirement: Workflow seed restore reports incomplete progress

When no valid mutable Workflow sidecar survives, restore SHALL derive Run identity and lifecycle from Timeline and SHALL mark agent usage incomplete. It SHALL NOT present the Spawn seed's zero usage as a complete cumulative count. Resume admission SHALL continue to reconcile agent calls from the durable journal.

#### Scenario: Completed run loses its progress sidecar
- **WHEN** a run has a durable terminal lifecycle but its sidecar is missing or invalid
- **THEN** restore retains the terminal status and reports `agent_usage_incomplete = true`, even if the seed contains zero agent rows and zero used calls.

#### Scenario: Valid progress sidecar survives
- **WHEN** a valid sidecar matches the Timeline frozen contract and lifecycle
- **THEN** restore uses its mutable progress and does not introduce an incomplete-usage marker solely because restore occurred.
