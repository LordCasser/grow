## ADDED Requirements

### Requirement: Context info results are bound to their requesting session view
The client SHALL apply an asynchronous context-info result only when its
session identity and session-binding epoch still match the requesting
AgentView; the check SHALL happen before updating live context state or any
scrollback/modal projection. A zero nonce SHALL retain scrollback intent, and
a nonzero nonce SHALL additionally match the open usage modal epoch as part of
that same pre-mutation validity check.

#### Scenario: Late result after rebinding is ignored
- **WHEN** a context-info request completes after the AgentView is rebound or unbound and rebound
- **THEN** neither live context state nor scrollback or modal state is changed

#### Scenario: Current success and failure retain their surface routing
- **WHEN** a result matches the session identity and binding epoch, and any nonzero nonce matches the open usage modal
- **THEN** success updates live context before routing to zero-nonce scrollback or matching modal, while failure routes only to that same surface

#### Scenario: Same-session reopen rejects the old modal result
- **WHEN** a nonzero-nonce context-info request completes after the usage modal is closed and reopened for the same session with a new modal nonce, even if the session binding is unchanged
- **THEN** neither live context state nor the reopened modal or scrollback state is changed
