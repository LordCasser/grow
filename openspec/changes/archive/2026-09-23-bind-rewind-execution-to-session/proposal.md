# Bind rewind execution feedback to its session

## Why

Rewind points and preview results carry read identity, but an executing rewind result only carries AgentId. The request can commit after that Agent view is unbound or rebound. The late result then truncates the new session's scrollback, changes its composer/inline editor, or opens an error overlay on the wrong session. Unlike a stale read, an execution result may represent committed disk and conversation effects, so silently discarding it would also be misleading.

## What changes

- Carry the source session ID and binding epoch with each execution effect and result.
- Reconcile a result against the exact source binding. When it no longer matches, leave the current view's transcript, overlay and draft alone and give a visible notice that the previous session needs reloading or verification.
- Close rewind-specific overlay/read state and pending inline resubmit on actual binding changes. Restore unsent composer and inline-edit text to the local view without automatically sending it.
- Treat a transport/parse failure after execution as an unknown outcome rather than proof that no rewind committed.

## Capabilities

- `client-surfaces`: rewind execution and session ownership.
