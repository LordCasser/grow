## ADDED Requirements

### Requirement: Subagent lifecycle projections are bounded metadata

Subagent lifecycle Grow notifications SHALL carry status and identity metadata without duplicating the child final output. The canonical child result and admitted completion receipt SHALL remain the source of that output. The parent actor SHALL persist and forward one stamped event under the existing independent-update and active-attempt gateway budget boundaries. A failed preview gateway reservation SHALL reject that exact attempt's canonical admission. Client-origin Grow notifications SHALL remain persist-only.

#### Scenario: Child finishes with a large answer during parent sampling

- **WHEN** a child result contains a large final answer and its parent has an active sampling attempt
- **THEN** the lifecycle Grow projection contains only metadata, its forwarded event uses the same event ID as its durable record, and the completion receipt continues to reference the full answer.

#### Scenario: Lifecycle preview gateway budget is exhausted

- **WHEN** the parent actor cannot reserve preview gateway credits for a child lifecycle projection
- **THEN** no uncredited lifecycle payload enters the gateway queue, that attempt fails preview admission, and canonical child lifecycle facts remain available for reconnect projection.
