# Design

The existing per-session `PreviewEventBudget` is the shared byte authority for auxiliary Grow delivery. Goal and Workflow senders serialize once, reserve credits for the persistence and gateway copies, then enqueue. A persistence command owns its reservation until the append completes; gateway completion owns the other reservation. The persistence actor marks a failed append or explicit reservation failure against its current attempt before the admission barrier or replay projection commit. This keeps the sender synchronous where Goal commit gates require nonblocking notification publication.

Exhaustion drops only the optional Grow presentation snapshot. Goal control snapshots and Workflow run manifests remain on their existing durable paths. The failed preview attempt cannot be admitted as successful. Outside an attempt, the next state emission or reconnect reconstructs UI state from those authorities.

Subagent progress is transient. Its publisher waits for gateway completion before producing another tick, but cancellation still interrupts that wait. No new durable progress record is introduced.
