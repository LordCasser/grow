# Long-term Goal runtime v6

Goal is one durable objective plus the right to request another turn after the session becomes idle. It is not a plan executor and owns no blackboard, task graph, planner/verifier child, or finalization phase.

## Durable control state

```rust
enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

struct GoalState {
    architecture_version: u8,
    goal_id: String,
    objective: String,
    status: GoalStatus,
    token_budget: Option<i64>,
    token_baseline: i64,
    parent_tokens_spent: i64,
    subagent_tokens_spent: i64,
    last_session_tokens_seen: Option<i64>,
    elapsed_ms: u64,
    created_at: String,
    updated_at: String,
    status_message: Option<String>,
}
```

Goal and the selected Behavior are written together in the versioned Timeline `Control` snapshot. The Timeline is the only persistence authority. A transition publishes UI state only after the durable append succeeds; failure restores the prior in-memory Goal. Create, edit, restart, complete, and clear therefore cannot expose a half-applied Goal/Behavior pair.

Goal architecture v6 deliberately rejects older snapshots. Planner/blackboard state is not projected or migrated because that would keep two lifecycle models alive.

## Lifecycle ownership

- The user creates, edits, pauses, restarts, budgets, and clears a Goal through `/goal`.
- The model sees only `create_goal`, `get_goal`, and `update_goal`.
- `update_goal(status=complete)` is valid only after the entire objective is achieved.
- `update_goal(status=blocked)` records a genuine impasse and stops automatic continuation.
- Usage or token limits stop the Goal without deleting its objective or accumulated usage.
- Completion and clear select Normal Behavior. Other stopped states keep Goal Behavior selected.

An unfinished Goal is exclusive with other special Behaviors. Goal does not own the foreground between turns, so user input, cancellation, and ordinary task execution still use the single session foreground/FIFO protocol.

## Idle continuation

The idle arbiter admits work in this order:

1. settle and durably record the current foreground terminal;
2. promote the oldest user input;
3. process higher-priority notification work;
4. if foreground and FIFO are still empty, allow an Active Goal continuation.

The Goal continuation is an internal regular turn with structured `PromptOrigin::GoalContinuation { goal_id }`. It is never inferred from prompt text or a prompt-id prefix. Admission rechecks foreground and pending input under the same state lock, so user input always wins a race with continuation.

Every continuation directive requires this order:

1. Audit the complete objective against conversation, workspace, tests, and other concrete evidence.
2. If fully satisfied, call `update_goal(status=complete)` and report the evidence.
3. Otherwise choose the next small, verifiable slice.
4. Track that slice with ordinary `todo_write` state and use `task` for bounded delegation or an independent check.
5. Verify the slice, then leave the Goal Active for the next idle continuation unless the full objective is complete or a genuine impasse exists.

Todo and task state is short-lived execution context. It is not copied into GoalState, cannot narrow the objective, and finishing it does not complete the Goal.

## Delegation and capabilities

A child spawned during Goal work receives an immutable `GoalView` and `SubagentOwner::Goal { goal_id }`. Descendants inherit that ownership. Goal-owned children may read `get_goal`, but lifecycle mutation tools are removed from their effective tool configuration. The primary session is the only Goal lifecycle writer.

The effective child tool surface remains the intersection of registered tools, Agent definition, Behavior policy, delegated capability, and user permission. Goal ownership adds an object-level restriction; it never expands capability.

Background task ids created by Goal work are remembered only to suppress late auto-wake notifications after the Goal stops. Their token usage is settled into the durable Goal budget, but progress ticks are not persisted as Goal updates.

## Observability

`GoalUpdated` projects only goal id, objective, lifecycle status, budget, usage, elapsed time, timestamps, and status message. Pager scrollback records lifecycle transitions—create, objective update, pause, block, usage limit, budget limit, restart, complete, and clear. It does not record streaming phase changes or task progress.

The Goal detail overlay is a read-only projection of that same state. It contains no task board or hidden planner state.

## Recovery invariants

- Active reloads re-arm idle continuation; stopped statuses remain stopped.
- A missing required Goal tool pauses an Active Goal with an actionable runtime-unavailable message.
- Complete receipts freeze Goal usage; later Normal turns are not charged.
- Graceful shutdown settles live delegated usage and checkpoints elapsed time before the persistence barrier.
- Fork/copy does not clone Goal runtime ownership.
- Rewind requires explicit Goal clear because prompt/file rewind has no prompt-indexed Goal snapshot.
- An Active Goal keeps the session resident even when foreground is idle; all stopped statuses may unload normally.
