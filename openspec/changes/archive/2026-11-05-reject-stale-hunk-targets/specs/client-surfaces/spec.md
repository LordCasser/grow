## ADDED Requirements

### Requirement: Hunk rejection detects an already changed disk target

Before reverting a displayed hunk or file batch, the hunk owner SHALL compare the actual target's complete content and existence against the current-content snapshot used for that action. A differing target SHALL produce an explicit stale-content failure without overwriting or deleting the external content or projecting the action as successful. All reject scopes SHALL share this boundary. This contract SHALL NOT be described as providing atomic compare-and-swap isolation against arbitrary non-cooperating writes after that comparison.

#### Scenario: External edit precedes a reject
- **WHEN** a file changes on disk after its displayed hunks are cached but before a reject reads the target
- **THEN** reject reports a stale conflict, preserves the changed file and leaves its pending hunk, turn membership and statistics uncommitted.

#### Scenario: Creation or deletion is stale
- **WHEN** reject would delete a newly tracked file that has since changed, or restore a deleted file that now exists
- **THEN** it rejects the stale action and preserves the external state.

#### Scenario: File, turn or all reject includes a conflict
- **WHEN** a batch encounters a file whose disk state differs from its displayed snapshot
- **THEN** that file fails without mutation or success projection, while any earlier successfully applied files remain represented by their actual result.

#### Scenario: Target still matches
- **WHEN** the actual content and existence match the displayed snapshot
- **THEN** rejection applies the selected reversal through the existing successful action path and counts it once.
