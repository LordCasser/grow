## ADDED Requirements

### Requirement: Goal Active queued inputs are directly editable and retractable

Pager SHALL let the user edit or remove a still-pending queued message while Goal is Active through the queue item's existing edit and cancel actions, without selecting Stop Goal or Stop Turn. It SHALL distinguish a confirmed pending item from an optimistic submission and an already-running item, and SHALL report the Shell's authoritative result instead of treating local UI mutation as success.

#### Scenario: Edit a pending message during Goal execution
- **WHEN** Goal is Active, another turn is running, and the user selects `[edit]` or the queue edit key for a confirmed pending message
- **THEN** Pager enters protected editing only after the Shell confirms the hold; the current turn, subagents, and Goal lifecycle remain unchanged.

#### Scenario: Retract a pending message during Goal execution
- **WHEN** Goal is Active and the user selects `[cancel]` or the queue delete key for a confirmed pending message
- **THEN** successful authoritative removal makes the message disappear and prevents its later execution without opening a Goal／turn stop choice.

#### Scenario: Pending submission or stale row
- **WHEN** the selected row is still an optimistic echo, has already started running, was removed elsewhere, or changed version before an edit or remove request is confirmed
- **THEN** Pager does not claim the operation succeeded; it reconciles with the authoritative queue and preserves any unsaved editing text for recovery or explicit user action.

#### Scenario: Failed edit save
- **WHEN** the Shell rejects or cannot durably admit edited content
- **THEN** Pager keeps the editing draft and shows a failure; it does not silently return to normal composer mode or send the old text as the edit result.

#### Scenario: Turn-stop shortcut remains distinct
- **WHEN** the user invokes Ctrl+C without selecting a queued item action
- **THEN** the existing current-turn／Goal interruption semantics remain unchanged; queued-message withdrawal is not inferred from a turn-stop gesture.
