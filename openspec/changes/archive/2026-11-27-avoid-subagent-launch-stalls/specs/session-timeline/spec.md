## ADDED Requirements

### Requirement: Subagent resume source validation yields the Shell executor

Shell SHALL keep its command executor available while validating a delegated resume source in storage. The derived child SHALL await the complete source checks before admission.

#### Scenario: Resume reads a large source lineage
- **WHEN** a derived child requests a stored resume source whose validation takes time
- **THEN** independent executor work can proceed during the storage read, and the child still requires the matching parent Spawn and Ended, authorized identity, and verified child result link before launch.
