## ADDED Requirements

### Requirement: Permission persistence uses the shared setting path
Pager SHALL retain PersistSetting for default permission preferences and NotifySessionPermissionMode for current-session changes, without the retired PersistPermissionMode policy and best-effort result variants.

#### Scenario: Default preference write fails
- **WHEN** default permission persistence fails
- **THEN** the shared setting coordinator applies its existing failure/rollback behavior.
