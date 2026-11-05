## ADDED Requirements

### Requirement: Released deliveries accept later canonical results

A historical Released review recorded before a result exists SHALL remain valid when that link later receives a canonical terminal result. Its absent result reference SHALL preserve the fact that release preceded delivery and SHALL NOT be rewritten to cite the later result. Released reviews with a result reference, Accepted reviews and NeedsRevision reviews SHALL retain exact result binding. Release SHALL NOT fabricate execution completion, a result reference or acceptance. Later terminal publication SHALL continue through canonical child-reference validation and existing pending-replan disposition.

#### Scenario: Result arrives after release
- **WHEN** a parent releases a live required delivery with no result reference and a valid canonical result arrives later
- **THEN** the result can commit to that link, the historical release remains unchanged, and pending parent updates are settled by the existing terminal transition.

#### Scenario: Release leaves an unknown execution unfinished
- **WHEN** a delivery is released while execution or result finalization remains unknown
- **THEN** release alone does not mark its link terminal, clear pending replans or satisfy the execution-disposition completion gate.

#### Scenario: Referenced review still requires an exact result
- **WHEN** an Accepted or NeedsRevision review lacks a terminal result, or a review supplies a mismatched result reference
- **THEN** plan validation rejects it; allowing a historical unbound release does not admit that invalid review.
