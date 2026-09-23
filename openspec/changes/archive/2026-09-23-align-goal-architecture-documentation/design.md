# Design

Read the current Goal implementation and `behavior-goal` specification before
editing the article. The implementation is authoritative for the persisted
field names and architecture version; the specification is authoritative for
the lifecycle and accounting boundaries.

The article change is deliberately limited to two factual corrections:

1. State that the current architecture is version 10 and that snapshots with
   another architecture version are rejected by `GoalTracker::validate_snapshot`.
2. Include `GoalTokenUsage`, `usage_breakdown`, `usage_incomplete`, and
   `usage_incomplete_acknowledged` in the illustrative durable state shape.

The rest of the article already describes the current lifecycle, consecutive
blocker audit, root-owned admission, delegated ownership, late usage, and
cache-inclusive accounting. No delta spec is needed, and no Cargo build is
needed for a documentation-only correction.
