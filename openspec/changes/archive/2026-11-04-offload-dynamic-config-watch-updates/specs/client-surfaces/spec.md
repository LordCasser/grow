## MODIFIED Requirements

### Requirement: Ready ACP service is not blocked by watcher registration

After a leader publishes readiness, ACP initialization SHALL be able to progress while filesystem watcher registration is pending. Watcher registration SHALL run outside the LocalSet executor. While registration is pending, its Tokio blocking task SHALL own the eventual watcher result. If shutdown begins first, the LocalSet SHALL stop awaiting registration promptly; the blocking task SHALL drop any watcher it later constructs when the OS call returns. The OS registration call itself is not cancellable, so this behavior does not bound blocking-worker or Tokio-runtime shutdown time.

#### Scenario: ACP initialization overlaps slow watcher registration

- **WHEN** the leader is ready and config or skills watch registration has not completed
- **THEN** the ACP agent and bridges continue to make progress while registration runs on the blocking pool

#### Scenario: Shutdown arrives during watcher registration

- **WHEN** leader cancellation arrives before a blocking watcher constructor returns
- **THEN** LocalSet startup returns without waiting for the constructor, while its blocking task retains ownership and drops any returned watcher when registration returns

#### Scenario: SessionActor starts while project discovery registration is pending

- **WHEN** a new SessionActor is spawned and its project discovery watcher is still registering filesystem watches
- **THEN** the actor continues initialization and processes its install-coordination command while registration runs outside the LocalSet; the blocking owner retains the watcher, performs later native refreshes, and drops it when session cancellation is observed

#### Scenario: Session runtime shuts down while project watcher registration is blocked

- **WHEN** a session's native project-discovery registration is blocked and session teardown has received its persistence writer acknowledgement
- **THEN** the dedicated session runtime and writer close without waiting for the native call; the host runtime retains the blocking worker and watcher owner until the call returns, then drops the watcher on that host blocking worker

#### Scenario: Host runtime shuts down while the project watcher is idle

- **WHEN** the host runtime begins shutdown while the native watcher worker is waiting for a change and the session observer has not been cancelled
- **THEN** a host-runtime guardian is dropped and cancels the native worker; the worker drops its watcher on its blocking thread and the session observer can finish without requiring another filesystem event

#### Scenario: Stdio agent finishes before skills registration

- **WHEN** the stdio agent I/O task completes while skills watcher registration remains blocked
- **THEN** the LocalSet returns the agent outcome promptly, while the blocking task retains ownership and drops any eventual watcher when registration returns

#### Scenario: In-process Pager gateway starts before watcher registration

- **WHEN** config or skills watcher registration is still pending in the in-process Pager worker
- **THEN** the ACP gateway is already polling and can initialize or serve sessions; config cwd path updates remain queued until the config watcher is ready

#### Scenario: Sessions arrive during in-process config watcher registration

- **WHEN** a session is created or a global/project MCP config changes before the in-process worker finishes registering its watchers
- **THEN** once the config watcher is ready and queued cwd interests are established, one all-session reload is queued through the live config consumer to reconcile each active session from its current effective config and auxiliary tool policy, preserving per-project scope and existing MCP connections for identical config

#### Scenario: Skills registration is blocked while config changes

- **WHEN** the config watcher is ready, skills watcher registration remains blocked, and global or project MCP configuration changes
- **THEN** the live config consumer applies the change without waiting for skills registration

#### Scenario: In-process worker terminates during watcher startup

- **WHEN** cancellation, a watcher registration error, or gateway completion ends the in-process worker after gateway polling starts
- **THEN** the worker flushes live session actors before returning, and any still-running blocking registration task retains and drops its eventual watcher result when the OS call returns

Dynamic session watch registration and release SHALL also execute outside the LocalSet, under one ordered blocking owner that retains and finally drops the watcher. LocalSet cancellation SHALL stop awaiting that owner promptly without claiming the native operation was cancelled. Closing the dynamic update channel SHALL NOT revoke independent global watches. Existing startup reconciliation SHALL wait for the initially queued cwd interests to be established.

#### Scenario: Dynamic registration overlaps protocol work
- **WHEN** a native watch or unwatch for a session path is blocked
- **THEN** unrelated ACP work continues, the update order remains intact and cancellation does not transfer native Drop back to the LocalSet.

#### Scenario: Dynamic update sender is closed
- **WHEN** no further session path updates can arrive while the runtime remains active
- **THEN** independent global and explicit watches remain owned until runtime cancellation.
