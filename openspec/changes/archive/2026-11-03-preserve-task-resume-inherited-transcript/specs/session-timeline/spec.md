## ADDED Requirements

### Requirement: Derived Task resumes retain inherited transcript presentation
A Task resume_from child SHALL persist the selected source branch's historical user, assistant, thinking and tool transcript, including visible content preceding compaction, together with its new Seed before publication. Model context and inherited presentation SHALL derive from the same validated canonical source cut. Presentation references retained in the active Surface SHALL be rebound to the new child's Seed; source response, cursor, ingress, control and execution identities SHALL NOT become the new child's authorities. Source runtime, route, usage, Plan and child lifecycle state projections SHALL NOT replace the new child's own state. Ordinary fresh/fork context selection SHALL retain its existing semantics.

#### Scenario: A resumed Task is viewed after reconnect
- **WHEN** a canonical completed or cancelled Task is used as resume_from and the derived child is later cold-viewed
- **THEN** the inherited transcript precedes the derived child's new prompt and output exactly once, under the derived display lineage.

#### Scenario: Source context has been compacted
- **WHEN** the selected source history contains user-visible messages no longer present in its active model Surface
- **THEN** the derived transcript retains that history while model context still uses only the validated active Surface.

#### Scenario: Source display echo and execution state
- **WHEN** the source transcript includes an admitted user display echo and source control/runtime presentation
- **THEN** the historical echo remains readable without a new ingress receipt, while source current-state projections and admission identities are not inherited.

#### Scenario: Inherited presentation cannot be committed
- **WHEN** source validation, history projection or staged child journal publication fails
- **THEN** the requested derived epoch fails without exposing a partially initialized transcript or changing the immutable source lifecycle.
