# Design: Preserve Plan state during Behavior selection

The BehaviorCoordinator already owns both Plan runtime and session-scoped state: the retained candidate, monotonic `last_plan_submission_id`, and completed-plan reference. Its `select_behavior` operation resets PlanRuntime and preserves the session-scoped fields. The durable snapshot must represent that same operation.

Create a temporary coordinator from the current Behavior snapshot, invoke its `select_behavior(target)`, and take the resulting snapshot. If the existing manual-selection rule dismisses a completed-plan reference, clear only that field in the pending snapshot. Persist the snapshot first; after its durable acknowledgement, apply the same selection and completion cleanup to the live coordinator. This preserves the existing durability ordering and does not add another state owner.

Retained candidate and submission identity remain present independently of PlanRuntime. Existing snapshot validation continues to reject Plan-only runtime residue under other Behaviors. GoalLifecycle selection keeps the existing completed-plan behavior. A failed durable write leaves live coordinator selection and Plan state unchanged.

The regression tests will cover the archived Deferred-to-Normal counterexample, a completed Plan switch, durable failure before live mutation, and reconstruction from the acknowledged Control followed by a new submission.
