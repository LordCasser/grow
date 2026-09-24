## ADDED Requirements

### Requirement: Queued input editing hold prevents admission to a turn

A confirmed edit hold on a still-pending user FIFO item SHALL prevent that item from being combined, promoted, or consumed while editing. The hold SHALL preserve the item's FIFO position; arbitration SHALL NOT skip a held head item to run a later user item, notification turn, or Goal continuation. This rule SHALL be independent of the current Behavior.

#### Scenario: Goal turn completes while the queued head is being edited
- **WHEN** an Active Goal turn finishes and the next user FIFO item has a confirmed edit hold
- **THEN** that item remains pending and unconsumed, no later work overtakes it, and the Goal remains Active without starting a continuation through the held item.

#### Scenario: Earlier FIFO work before a held item
- **WHEN** a later queued item is held for editing while an earlier unheld user item is pending
- **THEN** the earlier item may run first, but arbitration stops once the held item reaches the head.

#### Scenario: Hold races with promotion
- **WHEN** an edit-hold request and promotion target the same queued item at the same control boundary
- **THEN** either the hold is confirmed before promotion and blocks it, or promotion wins and the hold is rejected; the client SHALL NOT be told an already-running item is protected.

### Requirement: Completion of queued editing returns to ordinary FIFO arbitration

Saving a queued edit SHALL durably admit its replacement content before releasing the hold, preserving the row's FIFO position and replacing the old input identity exactly once. Discarding the edit SHALL release the hold without changing its admitted content. Removing the item SHALL durably dismiss its pending input before removing it. Each successful completion SHALL wake ordinary idle arbitration, not directly start or steer a turn.

#### Scenario: Save during an active turn
- **WHEN** the user saves an edited held item while another turn is running
- **THEN** the replacement waits in the same FIFO position and is eligible at the next normal send point after the current turn settles; it is not injected into the running turn.

#### Scenario: Save while idle
- **WHEN** the user saves an edited held head item while the session is idle
- **THEN** the next idle arbitration may immediately promote that replacement once, ahead of Goal continuation.

#### Scenario: Save admission fails
- **WHEN** replacement payload or input admission fails
- **THEN** the original queued input and hold remain intact, no replacement is sent, and the client receives a failure rather than a success projection.

#### Scenario: Discard or remove
- **WHEN** the user discards the edit or removes the still-pending item
- **THEN** discard makes the original item eligible at the next normal send point; successful removal makes that item ineligible forever without stopping the foreground turn or Goal.

#### Scenario: Editing client leaves
- **WHEN** the editing client disconnects without completing its edit
- **THEN** its transient hold is released, the original admitted input remains pending, and normal FIFO arbitration resumes without an indefinite Goal stall.
