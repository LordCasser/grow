## ADDED Requirements

### Requirement: System guidance bounds discretionary filesystem writes

The shared Mandatory Core SHALL instruct Agents to keep file creation and writes within the currently opened project directories, including the active worktree, by default, and to use /tmp or the platform's system temporary directory for disposable artifacts. It SHALL permit writing elsewhere only when the user explicitly requests that destination or the task clearly requires writing there, with external writes limited to necessary files. Convenience, an absolute path, or available filesystem access alone SHALL NOT be presented as sufficient justification. This guidance SHALL preserve existing permission and action-safety requirements and SHALL NOT introduce a blanket confirmation requirement or claim runtime filesystem isolation.

#### Scenario: Ordinary edits and disposable artifacts
- **WHEN** an Agent prepares ordinary task edits or temporary scripts without a required external destination
- **THEN** the shared system guidance directs task files to the current project/worktree and disposable artifacts to /tmp or the platform system temporary directory.

#### Scenario: Explicit or necessary external destination
- **WHEN** the user requests an output outside those directories or the task clearly requires an external configuration or installation write
- **THEN** the guidance allows the necessary files at that destination subject to existing authorization rules, without imposing an extra confirmation solely because of location.

#### Scenario: Available access is not task necessity
- **WHEN** a home directory, unrelated repository, or global configuration path is writable but the task does not require writing there and the user has not requested it
- **THEN** the guidance directs the Agent to avoid that write; an absolute path or convenience does not create an exception.

#### Scenario: Primary and delegated Agents use the same default
- **WHEN** Grow constructs a system head for a primary or subagent audience, including an Agent using full prompt composition
- **THEN** the same write-location guidance is present in Mandatory Core and is not removed by the authored role.
