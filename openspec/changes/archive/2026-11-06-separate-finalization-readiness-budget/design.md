# Design

Coordinator owns period deadlines and mutation/reconciliation attempt counts. Shell owns knowledge of its output I/O, SessionThread and parent writer incarnation. Current `advance_finalizations` increments attempts before invoking `ChildRunner::reconcile`; Shell `finalization::advance` then returns immediately for active I/O or a live child writer. Those are readiness observations rather than admitted recovery attempts.

Add to the existing ChildRunner trait:

```rust
fn finalization_ready(&self, _request: &SubagentRequest, _completion_data: &Self::CompletionData) -> bool { true }
```

It is synchronous and performs no file I/O, Timeline append, asynchronous query, recovery mutation dispatch, registry update or new deadline calculation. Shell uses the existing SessionThread retirement observer: after the actor exits it may request its persistence rail to stop, as the existing retirement path already does. That lifecycle signal neither acquires a replacement writer lease nor dispatches recovery writes, and it is not counted as a reconciliation attempt. The default preserves hosts without physical readiness constraints. Coordinator calls it after checking the existing in-flight/attempt/deadline gates and resolving the original completed child, immediately before incrementing attempts. False simply skips this tick. True admits one attempt; subsequent failures consume the existing budget. A race after observation does not authorize duplicate writes: reconcile still performs its full checks.

Shell implements the observation using the same effective parent as reconcile: a healthy currently resident parent can replace the stale stored parent; otherwise the retained parent must still be healthy. Output `output_write_in_flight` must be false and any retained child SessionThread must report fully finished (actor and persistence). A small crate-private ShellCompletionData predicate may expose these existing facts without adding state. Do not gate on usage/receipt ACK flags: reconciliation must observe their existing durable receipts rather than permanently blocking itself. No lease is acquired by readiness; real recovery still acquires and validates the existing exclusive lease and exact facts.

Tools owns the trait and scheduling rule. Its coder changes only tools coordinator/state/tests. Shell adapter follows in a separate bounded task with the exact signature above, once the provider exists. Parent owns review, integration, OpenSpec archive and backlog/docs. No writes overlap input or Activity tasks.

Tier 3 acceptance: more than three false-ready ticks followed by true within the original period dispatch exactly one successful reconcile; false-ready never invokes reconcile; readiness after the original deadline dispatches nothing until the existing explicit new-turn cycle; three actual admitted failing attempts remain bounded and read/wait do not reopen. Shell tests cover active output I/O, actor retired with persistence alive, both retired, missing/failed parent and a healthy replacement parent. Existing unknown-ACK and recovery lease tests remain authoritative. Parent independently reviews and verifies Tools+Shell integration after both deliveries.
