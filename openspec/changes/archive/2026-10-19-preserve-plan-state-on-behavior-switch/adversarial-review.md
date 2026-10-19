# Adversarial review

Review focus: whether the target Control can disagree with the coordinator again at another Behavior boundary, or accidentally revive Plan runtime.

- **Target snapshot source:** it is derived by reconstructing a temporary coordinator from the current Behavior snapshot and invoking the same `select_behavior` operation as the post-ack live update. This preserves retained-plan metadata and the monotonic submission counter while resetting PlanRuntime.
- **Durability ordering:** the clone is only used to build the proposed snapshot. The live coordinator is selected after the durable write succeeds. The failure regression asserts both the live snapshot and latest acknowledged Control remain unchanged.
- **Completion cleanup:** manual selections retain the established completed-plan dismissal behavior. GoalLifecycle selection does not set the manual clear flag and therefore retains the completion reference. Same-mode completion dismissal continues through its existing no-effect persistence path.
- **Snapshot validity:** only the independent retained candidate and counter cross Behavior boundaries. Artifact revision/hash, active submission, handoff and approval fields are reset by the coordinator selection semantics; existing non-Plan runtime validation remains in force.
- **Restore identity:** reconstruction uses the acknowledged Behavior snapshot and allocates the next submission from its counter. The regression checks that the new submission is 2 after a deferred submission 1.

Remaining validation risk: this change was not run through Cargo in this work item, per the parent task's instruction to centralize the Shell build. The named regression tests and existing Shell suite must be run by the parent before tasks are closed or the change is archived.
