## ADDED Requirements

### Requirement: Agent-local tasks end with their owner
Local background tasks that access an `MvpAgent` SHALL stop before the agent is destroyed, even when the agent's owning task is aborted while its `LocalSet` remains active.

#### Scenario: Abrupt owner task abort
- **WHEN** the task owning a leader agent is aborted with its local session supervisor and coordination work pending
- **THEN** those local tasks are cancelled before the agent's state is destroyed and cannot execute a later tick against the old agent.
