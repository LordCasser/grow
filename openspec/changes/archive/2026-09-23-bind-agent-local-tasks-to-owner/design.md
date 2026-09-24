# Design: Bind agent-local tasks to the agent owner

## Decision

Keep `MvpAgent` as the single owner of its local background work. A small task registry stores the `JoinHandle`s for every local task that captures `LocalRef<MvpAgent>`. The registration method prunes completed handles. `MvpAgent::drop` aborts registered handles before any fields are destroyed. On the single-threaded `LocalSet`, a task cannot concurrently poll during that drop; an aborted task is never polled again after the owner is gone.

The supervisor remains idempotent and continues periodic reaping while its owner exists. The coordinator, coordination publisher/heartbeat, and delayed announcement use the same registration boundary, because each captures a raw agent reference. The registry is not a general-purpose task supervisor and does not change session actor ownership.

## Verification

Exercise owner drop while delayed work is pending, existing supervisor tests, and the exact-test leader reconnect scenario that reproduced the crash. The in-process fixture does not kill its session actor OS thread when it aborts the agent's local task; if that leaves a writer lease, preserve it as a separate test-harness debt rather than claiming the reconnect behavior passed. Check the scoped shell and Pager builds, then validate and archive the OpenSpec change.
