## ADDED Requirements

### Requirement: Pager consumes recap admission outcomes
Pager SHALL await an asynchronous recap only after a valid accepted extension response. Disabled, rejected or invalid admission responses SHALL clear manual recap progress through the requesting session's failure path. Automatic failures SHALL remain silent and SHALL not clear a manual request's progress.

#### Scenario: Feature disabled after initialization
- **WHEN** shell responds to a manual recap with result.disabled=true
- **THEN** pager clears that session's manual progress rather than waiting for a recap notification

#### Scenario: Valid accepted response
- **WHEN** the extension response confirms ok=true and does not disable recap
- **THEN** manual progress remains until the asynchronous result arrives

#### Scenario: Automatic admission fails
- **WHEN** an automatic recap response is disabled, rejected or invalid
- **THEN** pager does not show a toast or clear existing manual progress
