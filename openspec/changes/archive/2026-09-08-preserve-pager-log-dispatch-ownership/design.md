# Design
Use one initialization owner containing the ACP sender and Tokio Handle captured in init; send_entries must dispatch through that owner rather than the producer thread runtime. Successful once-only installation controls timer spawn. Both flush variants check readiness before draining. Test local state or dependency-injected helpers rather than resetting global OnceLock or touching real logs. Inspect resulting ownership before choosing the smallest seam; avoid a new general logging framework.

# Limits
This does not claim all detached batches are awaited at shutdown, bounded transport memory, crash durability or retry after disconnect. These remain separate documented debts.

# Implementation refinement
The two process-global storage fields are grouped into a private Forwarder, with a OnceLock DispatchOwner containing sender and runtime. This is a concrete state owner, not a general logger abstraction. Local instances make real plain-thread and acknowledged transport tests independent of process-global initialization. initialize returns whether installation won; the public init returns immediately otherwise, before spawning its timer. Both flush paths share readiness-before-drain in take_entries. push retains the existing 16-entry threshold and releases the buffer lock before dispatch.

The flush_blocking documentation is corrected to describe only its current batch; no stronger shutdown guarantee is invented. Successful initialization still requires an entered live Tokio runtime, as before. Capturing a Handle does not make an already shut-down runtime execute tasks.
