## ADDED Requirements

### Requirement: Rewind operations reject historical load failure
Full-history rewind queries and mutations SHALL return a failed historical load rather than operate on an incomplete in-memory projection. The source and live points SHALL remain available for retry. Shell rewind and pending rewind recovery SHALL propagate this failure before applying their dependent effects.

#### Scenario: Historical read fails
- **WHEN** the deferred rewind source fails to read
- **THEN** full-history queries and mutations report failure without consuming the source or changing live points.

#### Scenario: Source becomes readable
- **WHEN** the same pinned source is repaired after a failed load
- **THEN** a subsequent load merges its historical points with live points using existing live-point precedence.

#### Scenario: Cancel requests an internal rewind
- **WHEN** cancellation needs to rewind its conversation but full rewind history cannot load
- **THEN** that internal rewind reports failure before committing its conversation rewind or truncating checkpoints; already committed cancellation terminal facts are retained.
