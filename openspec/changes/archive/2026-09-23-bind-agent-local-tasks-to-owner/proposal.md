# Change: Bind agent-local tasks to the agent owner

## Why

The leader reconnect scenario aborts the task owning `MvpAgent` while its `LocalSet` keeps running. A detached session supervisor still holds `LocalRef<MvpAgent>` and dereferences the destroyed agent on its next tick, causing SIGSEGV. Other agent-local tasks use the same raw-reference lifetime assumption.

## What Changes

- Register agent-local tasks that hold `LocalRef<MvpAgent>` with their owner and abort them when the owner is dropped.
- Keep normal resident-session supervision and coordination behavior while the agent lives.
- Run the isolated reconnect scenario to verify the crash no longer occurs; keep any independent reconnect failure in the backlog.

## Impact

Shell agent lifecycle and Pager leader reconnect. No persisted format changes.
