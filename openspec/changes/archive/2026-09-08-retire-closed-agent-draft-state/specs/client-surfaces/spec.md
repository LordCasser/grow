## ADDED Requirements

### Requirement: Closed agent drafts release only recoverable runtime state
When the last agent owning a local draft key closes, the runtime SHALL checkpoint pending content and release clean routing/load/cache state without deleting its saved draft. Failed checkpoints SHALL retain the latest content and respect retry backoff.

#### Scenario: Reopen a saved session draft
- **WHEN** a session is closed and later reopened under a new AgentId
- **THEN** its saved local draft is restored instead of being skipped by an obsolete loaded marker

#### Scenario: Checkpoint fails before reopen
- **WHEN** closing cannot save the latest draft and that key is reopened
- **THEN** retained latest content remains recoverable and is not replaced by stale disk content or an empty composer

#### Scenario: Shared draft ownership
- **WHEN** one agent closes while another agent still owns the same key
- **THEN** shared draft runtime state is not retired prematurely
