# Design: Stop session actors when their agent owner ends

## Decision

The owner already holds `SessionHandle` senders for primary and active child actors. `Drop` sends the existing `SessionCommand::Shutdown` to both sets before releasing their handles; the actor's current graceful-shutdown path closes admission and flushes its writer. Local tasks holding raw agent references are aborted first under the archived owner-lifetime change. `Drop` does not block the `LocalSet` waiting for OS threads.

The in-process test knows the session it is replacing. After aborting and draining the old generation's local tasks, it polls that session's actual writer lock with a deadline and proceeds only when it can take and release the lock. This is the same exclusion boundary the new generation must acquire. The check does not modify session data or bypass the lease. The fixture remains isolated in its exact-test subprocess.

## Verification

Prove that dropping an Agent enqueues shutdown for resident and child handles. Run the isolated reconnect scenario and assert history appears once and a new turn succeeds. Retain a separate backlog entry if the scenario exposes an unrelated failure after writer release.
