# Input routing and foreground ownership

Grow routes input by intent. Behavior changes model context and scheduling policy; it does not own admission.

## State ownership

The shell session actor owns both the only foreground slot and the explicit FIFO:

```text
ForegroundState = Idle | RegularTurn(AgentTask) | Compaction
```

Goal planner/verifier stages, watchers, and background tasks do not occupy this slot. Every regular turn has a structured origin/kind and exactly one durable `TurnCompleted`. A stale completion whose prompt id does not match the foreground owner cannot clear a newer turn.

The pager mirrors shell state. It does not infer ownership from Goal status, prompt text, token count, or prompt-id prefixes.

## Input classes

1. Plain Enter sends `QueuePrompt`. While idle the common gate may start it; while running it remains in the user FIFO.
2. Ctrl+Enter sends a steer request for the current regular turn.
3. Double Enter atomically converts the just-queued first row to steer.
4. Queue-row “Send now” invokes the same steer request.
5. Leading slash input is a Grow command and runs through the command plane. Control commands mutate state synchronously and never wait for model work or their own actor mailbox.

A successful Goal control that invalidates the running context (set/edit/enter/pause/clear) ends that exact foreground turn through normal cancellation. Read-only or non-invalidating controls (status/resume/budget), and rejected mutations, leave it running.

Steering includes `expected_turn_id`. The shell accepts it only if the identified regular turn is still foreground, then moves the queued payload into that same turn's input buffer. It never creates a replacement turn or another terminal. Compaction and idle state are not steerable.

## Idle admission

All regular work shares one admission sequence:

1. settle the exact foreground owner;
2. persist its single terminal;
3. promote the oldest user FIFO entry;
4. only if still idle, run Goal `on_idle` work.

Goal Executing/Summarizing rechecks foreground and FIFO while holding the same state lock before reserving a turn. This closes the race where a continuation and user input arrive together: the user wins.

Planning/Verifying may run concurrently as background stages because neither owns foreground. Their results commit only through a matching revisioned `StageLease`.

Background completion delivery follows the same ownership rule. A completion that satisfies an explicitly displaced wait is delivered exactly once; otherwise it enters the idle notification drain. The drain suppresses only task ids stamped as Goal-owned and continues to surface unrelated user/watcher work even while Goal is Active or after its Complete receipt remains loaded.

## Message identity

Each user submission has a stable `messageId` carried by the queue row, optimistic bubble, running notification, and ACP user-message echo. Pager reconciliation is keyed only by this identity:

- an optimistic bubble followed by an echo stays one bubble;
- replayed or duplicate echoes are idempotent;
- the echo may backfill server fields such as `promptIndex`;
- unrelated messages with identical or trim-equivalent text remain distinct.

No `skip_next_user_echo`, text matching, or adoption stash participates in routing.

## Goal interaction

Goal is an exclusive visible Behavior but not an exclusive foreground owner.

- Active Planning/Verifying keeps the Goal chip active while the session may be idle or run a user turn;
- ordinary messages add context without changing objective/plan revision;
- `/goal edit` revises the objective and returns the Goal to Planning;
- outside Goal Behavior, `/goal set` switches to Goal and creates the objective; inside Goal it is hidden and rejected;
- after selecting Goal with no objective, the next ordinary message is captured directly as the objective without a Pager-generated hidden command;
- an accepted `update_goal_plan` during Verifying cancels that lease's verifier and returns to Executing; a rejected update leaves the verifier untouched;
- pause keeps Goal Behavior but stops autonomous admission;
- complete or clear returns to Normal;
- an unfinished Goal rejects switching to another Behavior.

Goal continuation is an internal regular turn started by the idle hook, not a queue item or hidden control prompt. See [goal-continuation.md](./goal-continuation.md).

The persisted Markdown blackboard contains only shared human/Agent task state. Agent-only execution and tool policy is assembled in private runtime prompts. Goal detail opens on a compact checkbox/progress projection; `Enter`/`Space` opens the scrollable full Markdown board, `Esc` first returns to the summary and then closes the overlay. Pager never persists either display cache or navigation state.

Turn failure ownership follows the same structured-origin rule. A provider or
tool-definition error in a user turn remains that user's terminal and cannot
pause an otherwise healthy Goal planner/verifier. Only a
`GoalContinuation`/`GoalFinalization` failure enters the foreground Goal
degradation path; background stage failures are handled by their own lease and
retry counters.

## Compaction

Compaction is the only non-regular foreground owner. Manual and automatic compaction cannot overlap a regular turn or each other. While it owns foreground, user input may queue but cannot steer it. When compaction ends, the same FIFO-first idle arbiter resumes scheduling.

## Recovery

`TurnCompleted` is the durable lifecycle authority. Prompt response metadata is an idempotent secondary source. The pager watchdog may query shell prompt status when a submission appears stalled, but elapsed time alone never fabricates a terminal.

Cancellation settles the same foreground owner and then follows FIFO-first
idle admission. If no queued user/manual work claims the slot, an Active Goal
is woken immediately; Stop Turn Only cannot leave it dormant until the next
unrelated user message.

An Active Goal reload keeps its v3 persistent phase/plan/revisions and settled token counters, clears transient stage leases, reconciles Goal Behavior, and is resumed by the idle hook. Incompatible or malformed Goal state is diagnosed, deleted, and followed by a cleared projection after replay rather than migrated through legacy routing rules.
