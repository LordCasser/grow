# Input routing and foreground ownership

Grow routes input by intent. Behavior changes model context and scheduling policy; it does not own admission.

## State ownership

The shell session actor owns both the only foreground slot and the explicit FIFO:

```text
ForegroundState = Idle | RegularTurn(AgentTask) | Compaction
```

Goal's future continuation right, watchers, and background tasks do not occupy this slot. Every regular turn has a structured origin/kind and exactly one durable `TurnCompleted`. A stale completion whose prompt id does not match the foreground owner cannot clear a newer turn.

The pager mirrors shell state. It does not infer ownership from Goal status, prompt text, token count, or prompt-id prefixes.

## Input classes

1. Plain Enter sends `QueuePrompt`. While idle the common gate may start it; while running it remains in the user FIFO. Under the non-default `follow_up_behavior = "steer"`, a plain-Enter follow-up that arrives while a regular turn is running is auto-promoted into that turn through the same mid-turn interjection entry point as Ctrl+Enter; only a regular foreground turn is promotable — idle or compaction states, synthetic prompts, and bash or structured prompts are not promoted and stay on the FIFO.
2. Ctrl+Enter sends a steer request for the current regular turn.
3. Double Enter atomically converts the just-queued first row to steer.
4. Queue-row “Send now” invokes the same steer request.
5. Leading slash input is a Grow command and runs through the command plane. Control commands mutate state synchronously and never wait for model work or their own actor mailbox.

A successful Goal control that invalidates the running context (set/edit/enter/pause/clear) ends that exact foreground turn through normal cancellation. Read-only or non-invalidating controls (status/restart/budget), and rejected mutations, leave it running.

Steering includes `expected_turn_id`. The shell accepts it only if the identified regular turn is still foreground, then moves the queued payload into that same turn's input buffer. It never creates a replacement turn or another terminal. Compaction and idle state are not steerable.

The turn terminal is also a steering-scope fence. A residual steer that missed the sampler's final safe-point drain is discarded on completion or cancellation; it can never leak into a later user turn or Goal continuation merely because Goal remains active. The fence is decided by turn identity: an explicit steer's residual is still discarded, because an explicit steer belongs to the exact turn it named; an auto-promoted follow-up's residual returns to the front of the user FIFO at the terminal boundary as a brand-new turn (keeping its original prompt identity) instead of silently dropping user input. Neither kind ever leaks into a successor turn's steering buffer.

## Idle admission

All regular work shares one admission sequence:

1. settle the exact foreground owner;
2. persist its single terminal;
3. promote the oldest user FIFO entry;
4. only if still idle, run Goal `on_idle` work.

An Active Goal rechecks foreground and FIFO while holding the same state lock before reserving its continuation turn. This closes the race where a continuation and user input arrive together: the user wins.

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

- Active Goal keeps the Goal chip active while the session may be idle or run a user turn;
- ordinary messages add context without replacing the objective;
- `/goal edit` revises and reactivates the same long-lived Goal while preserving usage;
- outside Goal Behavior, `/goal set` switches to Goal and creates the objective; inside Goal it is hidden and rejected;
- after selecting Goal with no objective, the next ordinary message is captured directly as the objective without a Pager-generated hidden command;
- pause keeps Goal Behavior but stops autonomous admission;
- restart re-arms paused, blocked, or usage-limited Goals;
- complete or clear returns to Normal;
- an unfinished Goal rejects switching to another Behavior.

Goal continuation is an internal regular turn started by the idle hook, not a queue item or hidden control prompt. See [goal-continuation.md](./goal-continuation.md).

Goal does not persist a plan or task graph. Each continuation audits the full objective, then uses ordinary `todo_write` and `task` execution context for the next small slice. Goal detail renders only the durable objective, lifecycle status, usage, elapsed time, and status message; Pager never persists its display cache or navigation state.

Turn failure ownership follows the same structured-origin rule. A provider or
tool-definition error in a user turn remains that user's terminal and cannot
pause an otherwise healthy Goal. Only a structured `GoalContinuation` failure
enters the Goal degradation path.

## Compaction

Compaction is the only non-regular foreground owner. Manual and automatic compaction cannot overlap a regular turn or each other. While it owns foreground, user input may queue but cannot steer it. When compaction ends, the same FIFO-first idle arbiter resumes scheduling.

## Recovery

`TurnCompleted` is the durable lifecycle authority. Prompt response metadata is an idempotent secondary source. The pager watchdog may query shell prompt status when a submission appears stalled, but elapsed time alone never fabricates a terminal.

Cancellation settles the same foreground owner and then follows FIFO-first
idle admission. If no queued user/manual work claims the slot, an Active Goal
is woken immediately; Stop Turn Only cannot leave it dormant until the next
unrelated user message.

Image-input capability recovery is session state, persisted with the existing
tool resources in `resources_state.json`. The negative cache key is the model
name, API backend, and endpoint fingerprint; absence means unknown and permits
an image attempt. A cache entry is written only when an image-bearing request
receives an API HTTP 400 that explicitly rejects `image_url`, `input_image`, or
another image content type in favor of text. Decode errors, size/dimension
limits, content-policy failures, and generic 400s remain terminal errors and do
not teach model capability.

After an explicit rejection, the shell groups all canonical `User` and
`ToolResult` images by message. Within one bounded recovery operation, a
configured auxiliary runtime that is distinct from the rejected runtime gets
one description request per group, with attachment count and order in its
prompt. Successful groups become sanitized description blocks. Failed groups,
or every group when no auxiliary runtime is usable, become explicit removal
text. Only the auxiliary runtime's own explicit image HTTP 400 enters its
negative cache; resolution, transport, timeout, and empty-response failures do
not teach capability.

The chat-state actor compares message position plus image-group fingerprint,
removes every current image, and persists the entire rewrite atomically before
acknowledging it. Message order, prompt indices, synthetic metadata, ordinary
text, and tool call/result pairing remain unchanged. The turn then rebuilds an
image-free request and resubmits through a dedicated recovery outcome that
does not consume ordinary retry budget or emit a failed turn. An image-count
assertion plus request-copy strip is the final loop guard; the canonical actor
rewrite is the normal path.

Canonical removal is session-wide and is not reversed by switching models.
User attachment assets and original `read_file` source files remain available,
so reading them again can create new image messages. If the active runtime is
already in the negative cache, newly appended images enter the same
conversion/removal operation before sampling instead of provoking another
400. Capability remains isolated by model/backend/endpoint, while the history
rewrite is shared by the session. `ImageDropped` reports whether images were
all converted, all removed, or mixed. No OCR backend participates in this
recovery path.

An Active Goal reload restores the v7 objective, definition revision, lifecycle status, budget, settled usage, and elapsed time from the same Timeline Control snapshot as Behavior, then re-arms idle continuation. Goal owns no persisted plan, board, planner phase, or stage lease. Older Goal architectures and invalid v7 snapshots are rejected without migration instead of reviving a second lifecycle model.
