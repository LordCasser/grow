# Design

The durable transition currently calls `BehaviorSnapshot::selected(Goal)` and patches back only completed-plan metadata. That loses a deferred candidate and does not consistently preserve the Plan submission counter.

Build the target by passing the current `BehaviorSnapshot` to `BehaviorCoordinator::from_snapshot`, calling `select_behavior(Goal)`, then taking `snapshot()`. This uses the same transition semantics as the live update: Plan runtime fields reset, while retained candidate, completed-plan reference, and monotonic submission counter remain under the session owner. Continue persisting this target before selecting Goal in the live coordinator.

Use the same helper for Goal lifecycle transitions caused by definition edits. Regression coverage should exercise a deferred candidate on first activation and on reactivation, compare live and latest durable snapshots, restore the coordinator from that Control, and verify the next submitted Plan has an identity greater than the persisted counter.
