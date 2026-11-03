## ADDED Requirements

### Requirement: Task-cancellation entry scopes preserve target ownership

The terminal-command cancellation entry SHALL operate only on the injected terminal backend and SHALL NOT inspect or call a subagent backend. The mixed `kill_task` entry SHALL retain terminal-first behavior when a terminal is available and SHALL permit cancellation through the session-bound subagent backend when no terminal is available. SubagentBackend SHALL remain the authority for child-source authorization and idempotent lifecycle outcomes. Both entries SHALL preserve actual killed, already-exited, not-found and unauthorized results; a terminal match SHALL take precedence over a same-ID child.

#### Scenario: Terminal-only entry misses a command ID

- **WHEN** `kill_terminal_command` has a terminal and a subagent backend, and the terminal reports `NotFound` for the requested ID
- **THEN** it returns the terminal not-found result without reading or calling the subagent backend, and does not claim that no subagents exist

#### Scenario: Mixed entry runs without a terminal

- **WHEN** `kill_task` has only a session-bound subagent backend and the requested child is authorized
- **THEN** it requests cancellation through that backend and returns its actual killed or already-exited result

#### Scenario: Foreign child is rejected

- **WHEN** the mixed entry requests a child outside the backend caller's authorized source scope
- **THEN** the backend authorization failure is returned as unauthorized and no successful cancellation result is reported

#### Scenario: Child has already finished

- **WHEN** the backend reports that the requested child has already finished
- **THEN** the mixed entry returns `already_exited` and does not claim a new cancellation

#### Scenario: Terminal and child share an ID

- **WHEN** the mixed entry has both resources and the terminal reports `Killed` or `AlreadyExited` for an ID also known to the child backend
- **THEN** it returns the terminal result and does not send a child cancellation request
