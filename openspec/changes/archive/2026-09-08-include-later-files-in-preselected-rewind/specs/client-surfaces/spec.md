## MODIFIED Requirements

### Requirement: Rewind file modes consider the affected checkpoint range
Rewind mode selection SHALL consider file changes at the target prompt and all later checkpoints when determining file-mode availability. It SHALL recompute the same range when returning from preview. Inline edit SHALL retain its existing FilesOnly exclusion.

#### Scenario: Later checkpoint has changes
- **WHEN** the selected checkpoint has no own file changes but a later checkpoint does
- **THEN** ordinary rewind offers FilesOnly for the selected target, including after back navigation.

#### Scenario: Changes precede the target
- **WHEN** file changes exist only before the target
- **THEN** those earlier changes do not enable FilesOnly for that target.

#### Scenario: Inline resubmission
- **WHEN** inline edit selects a target with later file changes
- **THEN** file changes are recognized while FilesOnly stays excluded.

#### Scenario: Preselected target bypasses the picker
- **WHEN** points finish loading for a preselected rewind target, including an inline edit target
- **THEN** file-mode eligibility includes all checkpoints at or after the resolved target and retains the inline FilesOnly exclusion.
