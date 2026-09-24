## Context

`grow/workflows/list` resolves a session handle asynchronously, then calls `workflow::registry::list_workflows` inline. That call discovers project and user workflow directories and reads candidate scripts. The skill reload helper already runs comparable discovery in `spawn_blocking` under a semaphore and a deadline, retaining the permit in the worker because filesystem calls cannot be cancelled.

## Goals / Non-Goals

**Goals:** Keep workflow discovery off Tokio runtime workers, cap outstanding scans at one, and make capacity wait plus scan time fit within five seconds.

**Non-Goals:** Changing workflow discovery rules, caching results, interrupting filesystem calls, changing response shape, or changing synchronous workflow execution paths.

## Decisions

- Reuse the bounded blocking-worker pattern used by skill reload, with a workflow-list-specific semaphore so an unresponsive filesystem cannot create an unbounded queue of blocking scans. The deadline wraps both semaphore acquisition and worker completion.
- Keep the owned semaphore permit inside the blocking closure. Dropping or timing out the request detaches an in-flight blocking task; its permit remains held until the scan truly exits.
- Preserve discovery failure as an RPC error. Returning an empty list would make a timeout indistinguishable from a valid workspace with no workflows.
- Verify cancellation and timeout with an injected blocking closure controlled by synchronization primitives, rather than relying on a slow real filesystem.

## Risks / Trade-offs

- A synchronous filesystem call cannot be forcibly stopped; a wedged scan can hold the single workflow slot for the process lifetime. This bounds resource growth while later requests fail within their deadline.
- Runtime shutdown follows Tokio's blocking-task shutdown policy; a detached blocking scan may delay graceful runtime shutdown until the operation exits. The implementation introduces no detached task beyond the blocking worker already required to isolate synchronous I/O.

## Migration Plan

No persistent data or protocol migration is needed. Deploy the handler change directly; callers already accept RPC errors. Revert the handler/helper change to restore inline listing if necessary.
