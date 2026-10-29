# Unblock ACP during watcher registration

## Why

The leader marks ACP forwarding ready before entering its LocalSet. After queuing the ACP agent and bridge tasks, it synchronously registers config and skill filesystem watches before the LocalSet first polls those tasks. On macOS, `notify` FSEvents registration may synchronously wait for its runloop thread, delaying ACP initialization even though the leader has advertised readiness.

## What Changes

- Construct config and skill watchers on Tokio's blocking pool so the LocalSet can serve ACP initialization while OS watch registration is pending.
- In the in-process Pager worker, start ACP gateway polling before awaiting config and skill watcher registration, and queue cwd path updates until the config watcher is ready.
- As soon as the config watcher is ready and queued cwd interests are registered, start its live consumers and queue one existing all-session MCP catalog reload; skills registration proceeds independently while config and policy updates remain live.
- Keep constructed watcher resources owned by their Tokio blocking task until registration returns. If shutdown arrives during registration, let the LocalSet return promptly; the blocking task drops any resulting watcher when the OS call returns.
- In stdio mode, let agent I/O completion end startup promptly if it wins the race against pending skills watcher registration.
- Flush live session actors before the in-process worker exits after cancellation, registration failure, or early gateway completion.
- Keep the existing watch plan, reload behavior, readiness signal, and shutdown cancellation semantics.

## Capabilities

### Modified Capabilities

- `client-surfaces`: ready leader ACP initialization can make progress independently of filesystem watcher registration.

## Impact

The change covers watcher registration in `shell/src/agent/app.rs` and the in-process worker in `pager/src/acp/spawn.rs`. It reuses the shell registration completion helper. It does not claim that every observed ACP timeout had the same operating-system cause, or that an in-flight OS registration call can be cancelled or bounds Runtime shutdown.
