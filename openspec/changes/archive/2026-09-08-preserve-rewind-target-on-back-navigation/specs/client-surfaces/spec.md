## ADDED Requirements

### Requirement: Rewind back navigation preserves the active target
Returning to rewind mode selection SHALL preserve the current phase's explicit target, including when the cached checkpoint list is missing. Fallback selection SHALL be used only when the current phase does not carry a target.

#### Scenario: Older target preview
- **WHEN** an older checkpoint is selected and its preview confirmation returns to mode selection
- **THEN** the original target remains selected rather than changing to the newest checkpoint.

#### Scenario: Cached list unavailable
- **WHEN** confirmation retains a target but cached checkpoint metadata is unavailable on return
- **THEN** mode selection retains that target.

#### Scenario: Conversation-only confirmation
- **WHEN** the target-zero conversation-only confirmation returns to mode selection
- **THEN** the target remains zero even if newer checkpoints exist.
