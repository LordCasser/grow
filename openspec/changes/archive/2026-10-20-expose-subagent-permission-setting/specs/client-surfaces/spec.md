## ADDED Requirements

### Requirement: Settings exposes independent subagent permission policy
Settings SHALL expose a persistent Agent-category Subagent permission mode choice with Ask, Auto and Always approve, defaulting to Auto and visible in fullscreen and Minimal. It SHALL reflect `[subagents].permission_mode` independently of the primary permission default and SHALL preserve all three choices even when primary Auto selection is gated off. Changes SHALL use ordered setting persistence and rollback, indicate restart is required, and SHALL NOT change active-session permission or emit session permission notifications.

#### Scenario: Select and reopen child permission policy
- **WHEN** the user selects Ask or Always approve in the subagent permission picker and saves successfully
- **THEN** reopening Settings shows the selected child policy, while the primary default and active-session permissions retain their values.

#### Scenario: Reset and gated primary Auto
- **WHEN** the primary Auto picker gate is off and the user resets the child permission setting
- **THEN** the child setting resets to Auto through its own save path and Auto remains selectable there.

#### Scenario: Sequential writes fail
- **WHEN** multiple child permission selections are queued and their saves fail
- **THEN** earlier failure does not overwrite a newer selection and final failure restores the last confirmed child policy.
