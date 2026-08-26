# Long-term Goal runtime v9

Goal is one durable objective plus the right to request another turn after the session becomes idle. It is not a plan executor and owns no blackboard, task graph, planner/verifier child, or finalization phase.

## Durable control state

```rust
enum GoalStatus {
    Active,
    Paused,
    Blocked,
    BudgetLimited,
    Complete,
}

struct GoalState {
    architecture_version: u8,
    goal_id: String,
    definition_revision: u64,
    objective: String,
    status: GoalStatus,
    token_budget: Option<i64>,
    tokens_used: i64,
    elapsed_ms: u64,
    created_at: String,
    updated_at: String,
    status_message: Option<String>,
    blocked_audit: Option<GoalBlockedAudit>,
}
```

Goal and the selected Behavior are written together in the versioned Timeline `Control` snapshot. The Timeline is the only persistence authority. A transition publishes UI state only after the durable append succeeds; failure restores the prior in-memory Goal. Create, edit, restart, complete, and clear therefore cannot expose a half-applied Goal/Behavior pair.

Goal architecture v9 deliberately rejects older snapshots. `goal_id` is the stable identity of one long-lived Goal and changes only after explicit clear plus create. `definition_revision` advances when the user-controlled objective or token budget changes and invalidates stale continuation directives. Keeping identity and revision separate means an edit can cancel old execution without orphaning usage from work admitted before the edit; pause/restart likewise preserves the owner so late terminal receipts settle against the Goal that admitted them. Restart only resets stopped lifecycle state and the blocked audit. Lifecycle and accounting checkpoints cannot invalidate model context. `tokens_used` is cumulative model consumption—uncached input plus output from each admitted primary-Agent call and each acknowledged usage fold from a Goal-owned child. It never derives from current context pressure, so compaction, pruning, provider anchors, and request shadows cannot decrease or replay the budget. Planner/blackboard state is not projected or migrated because that would keep two lifecycle models alive.

## Lifecycle ownership

- The user creates, edits, pauses, restarts, budgets, and clears a Goal through `/goal`.
- Creating a different Goal requires explicitly clearing the existing Goal,
  including a completed one; completed state is never silently overwritten.
- The model-facing Goal lifecycle surface is only `create_goal`, `get_goal`, and `update_goal`.
- `update_goal(status=complete)` is valid only after the entire objective is achieved.
- `update_goal(status=blocked, blocker=...)` records one prompt-indexed impasse. Only the same blocker reported on three consecutive Goal turns stops automatic continuation; earlier reports remain Active, and another blocker or an intervening turn resets the count.
- The explicit token budget can stop the Goal without deleting its objective or accumulated usage; Grow does not guess a durable usage-limit state from provider 429/503/529 responses.
- Only Active selects Goal Behavior. Pause, block, budget limit, and completion release to Normal. Clear deletes Goal state but preserves another Behavior already selected while the Goal was stopped.

An Active Goal cannot coexist with another special Behavior. A stopped Goal is durable thread state orthogonal to Behavior, and can be edited, restarted, re-budgeted, or cleared while Normal, Clarify, Plan, or Workflow is selected. Goal does not own the foreground between turns, so user input, cancellation, and ordinary task execution still use the single session foreground/FIFO protocol.

## Idle continuation

The idle arbiter admits work in this order:

1. settle and durably record the current foreground terminal;
2. promote the oldest user input;
3. consume pending durable notification receipts that are allowed to start a turn;
4. if foreground and FIFO are still empty and no autostart notification remains, allow an Active Goal continuation; active-turn-only checkpoint receipts join that Goal turn before its first sample.

The Goal continuation is an internal regular turn with structured `PromptOrigin::GoalContinuation { goal_id }`. It is never inferred from prompt text or a prompt-id prefix, and it is not an auto-wake substitute for background completion. The idle arbiter drains receipts before calling the Goal driver; the driver then rechecks foreground and user FIFO under the same state lock, so user input and receipts always win a race with continuation.

Every continuation directive requires this order:

1. Treat completion as unproven and audit every concrete requirement in the complete objective against authoritative current evidence.
2. If fully satisfied, call `update_goal(status=complete)` and report the evidence.
3. Otherwise choose the next small, verifiable slice.
4. Track that slice with ordinary `todo_write` state and use `task` for bounded delegation or an independent check.
5. Verify the slice, then leave the Goal Active for the next idle continuation unless the full objective is complete or a genuine impasse exists.

Todo and task state is short-lived execution context. It is not copied into GoalState, cannot narrow the objective, and finishing it does not complete the Goal.

The full continuation directive is a durable Timeline message. Provider and compaction request assembly expand only the newest directive matching the active `goal_id + definition_revision`; every older Goal directive is projected as a small shadow in the same user-message position. This keeps turn/tool chronology intact, prevents old objectives and completion audits from remaining simultaneously active or leaking back through a summary, and leaves the canonical Timeline untouched for debugging. Paused, completed, cleared, and superseded Goal directives are all shadowed.

## Delegation and capabilities

A child spawned during Goal work receives an immutable `GoalView` and `SubagentOwner::Goal { goal_id }`. Descendants inherit that ownership. Goal-owned children may read `get_goal`, but lifecycle mutation tools are removed from their effective tool configuration. The primary session is the only Goal lifecycle writer.

The effective child tool surface remains the intersection of registered tools, Agent definition, Behavior policy, delegated capability, and user permission. Goal ownership adds an object-level restriction; it never expands capability.

Background task ownership is stamped with the admitted `goal_id` and survives lifecycle changes long enough to classify late receipts. Evidence matching the current Active Goal remains pending for that Goal continuation. Evidence owned by an older, paused, blocked, budget-limited, complete, or cleared Goal is durably `Dismissed(reason=goal_owned_autostart)` and its payload is reclaimed after commit; it can never autostart a Normal turn. The owner rule applies uniformly to task, monitor, subagent, and Workflow completion sources. A Goal-owned child's acknowledged usage-ledger fold settles its uncached input plus output exactly once into the durable Goal budget before terminal presentation. Live progress remains a context-pressure diagnostic and is never treated as cumulative consumption or persisted as a Goal update.

Every terminal Goal transition first retires the exact producing prompt and then sweeps the stable `goal_id`. The prompt retirement is the coordinator epoch's admission tombstone: a detached child spawn that races after the sweep is rejected by its parent prompt identity instead of recreating work under the stopped Goal. Process restart creates a new coordinator epoch and cannot resurrect an old in-process spawn future. Automatic turn-error and token-budget stops carry the just-settled prompt id through this same path; explicit lifecycle controls use their command authority boundary and never maintain a second cancellation registry.

## Observability

`GoalUpdated` projects only goal id, objective, lifecycle status, budget, usage, elapsed time, timestamps, and status message. Pager scrollback records lifecycle transitions—create, objective update, pause, block, budget limit, restart, complete, and clear. It does not record streaming phase changes or task progress.

The Goal detail overlay is a read-only projection of that same state. It contains no task board or hidden planner state.

## Recovery invariants

- Active reloads re-arm idle continuation; stopped statuses remain stopped and restore with Normal unless another orthogonal Behavior is already selected.
- The Goal runtime requires `create_goal`, `get_goal`, `update_goal`, and
  `todo_write`. A missing required tool pauses an Active Goal with an actionable
  runtime-unavailable message; `task` remains optional bounded delegation.
- Usage is charged by the immutable owner captured at admission, even when settlement follows pause or completion. Later Normal turns have no Goal owner and are not charged.
- Graceful shutdown checkpoints already-accounted usage and elapsed time before the persistence barrier; it never guesses delegated consumption from a live context watermark.
- Fork/copy does not clone Goal runtime ownership.
- Rewind requires explicit Goal clear because prompt/file rewind has no prompt-indexed Goal snapshot.
- An Active Goal keeps the session resident even when foreground is idle; all stopped statuses may unload normally.
- A permanent durable turn-boundary failure ends the session writer epoch after local cleanup. Recovery closes the incomplete turn before any Goal continuation is re-armed.
