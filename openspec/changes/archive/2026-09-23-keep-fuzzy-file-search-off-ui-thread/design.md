# Design: Keep fuzzy file-search worker teardown off the UI thread

The UI-facing daemon shares a short mutex containing at most one pending restart flag and one pending query tuple. Submissions merge into that state, then `try_send(())` a wake on a capacity-one channel. A full channel means a wake already exists. When the worker wakes, it takes the state, applies the latest restart first and then the latest query; obsolete intermediate queries need no matcher work. The query's synchronously assigned ID remains attached to its eventual snapshot.

Drop sets a stop flag, replaces pending work with stop, cancels the current walker and sends a nonblocking wake. The `JoinHandle` is detached; the worker owns the matcher and its walk lifecycle and exits after the current bounded work step observes stop or completes. This removes UI-thread waiting but does not claim a wall-clock deadline for a kernel/filesystem operation already in progress. A refused worker spawn retains the existing disabled behavior.

The worker generation jumps to at least the latest query ID when it consumes a coalesced request, because existing Pager and workspace callers keep per-query generation floors. Workspace status polling also compares the result's query ID to its current request; a generation alone can be higher from ticks of an older query and is not sufficient identity.

Verification covers pending merge, nonblocking submit/drop with a parked worker, final query identity and disabled mode. The shared search worker backlog retains unproven slow-filesystem exit latency.
