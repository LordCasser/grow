# Input routing and foreground ownership

Grow routes input by intent. Behavior changes model context and scheduling
policy; it does not decide whether input is admitted.

## State ownership

The shell session actor owns the foreground slot and the authoritative user
queue. Exactly one of the following may own the slot at a time:

- a finite prompt turn;
- a Goal planner/verifier stage;
- an exclusive history operation such as manual compaction;
- nobody (`Idle`).

`current_prompt_id` is correlation metadata. It is never a second busy-state
authority. Every foreground start has one prompt/operation identity and must
produce exactly one actor-committed terminal. Worker completions with an old
identity are stale and cannot clear or overwrite a newer foreground owner.

The pager mirrors, but does not predict, this state. Sending a prompt records a
submission; it becomes running only after the shell reports the matching
`running_prompt_id`. `PromptResponse` and durable `TurnCompleted` converge on
one prompt-id-guarded finalizer. The first terminal wins; later copies may add
metadata but cannot finish the turn, draw a second marker, adopt another turn,
or drain the queue again.

## Input classes

1. Plain Enter creates a user prompt in the shell-owned FIFO. Queue version,
   owner, edit, reorder, remove, clear, and combine-on-promote semantics do not
   depend on Behavior.
2. Send now under an active Goal is foreground steering. The actor atomically
   removes the corresponding queued row and transfers its payload to the
   active turn inbox. Outside Goal it retains cancel-and-promote semantics.
3. Leading-slash input is always a Grow command. Every shell-owned command
   goes through the `grow/commands/execute` command plane — including while
   idle — and command text is never re-queued as a user prompt. Local commands
   stay local; commands that intentionally start model work create
   command-origin work. Raw slash text is never a user message and unknown
   commands are errors.
4. Control commands change state synchronously and return a structured system
   log. A command handler must never wait for an event handled by the same
   actor mailbox. Planner, sampler, persistence completion, replay flush, and
   compaction are actor effects, not inline command work.

## Goal lifecycle versus prompt turns

Goal is a scheduler across finite turns, not one root prompt turn. A Goal cycle
contains an optional planning stage, one implementer turn, deterministic
cycle-end gates, and an optional verifier/classifier stage. Goal continuation
is scheduled only after explicit user work and pending manual compaction.

### Goal stages

Planning, verification, and classifier work are background Goal stages with
lease-scoped proposals. The turn-end drain schedules `completed: true`
verification proposals as stages instead of awaiting model work inline, and a
stage completion commits through the actor mailbox (a `GoalStageCompleted`
event) only while its Goal id, definition revision, autonomy generation, and
stage id still match the current lease. A paused, revised, cleared, or
completed Goal can receive late diagnostics but never late formal writes.

The non-workflow foreground path is a finite cycle: one implementer turn per
prompt, verification scheduled as background stages by the turn-end drain, and
the next continuation queued as a fresh `GoalSummary` prompt only after the
current prompt reaches its durable terminal. The workflow-engine path
(`background_workflows_enabled`) keeps `run_goal_round_end` as an in-turn
evaluator: a Continue verdict injects the next directive inside the same turn
task, and its `completed` proposals are still scheduled as stages by the
turn-end drain.

Goal status and Behavior are orthogonal:

- `set` selects Goal Behavior and changes the definition revision;
- `budget` changes only the resource constraint;
- `pause` stops autonomy and invalidates the current autonomy generation;
- `resume` changes lifecycle state immediately but never inserts a GoalControl
  prompt ahead of already-queued user input;
- `clear` and verifier-confirmed completion leave Goal Behavior;
- completion returns to Normal while retaining a display-only receipt;
- selecting a special Behavior after completion retires that receipt.

Selecting Goal with no objective makes the first ordinary text equivalent to
hidden `/goal set <text>`.

### Paused Goal interaction

Paused Goal never blocks prompt promotion. A paused user turn receives current
Goal context plus a paused-interaction directive, but not the autonomy
directive. It may answer, accept corrections, and perform explicitly requested
bounded work. It cannot schedule another Goal cycle or complete the Goal. The
turn ends normally and the Goal remains paused.

### Active Goal steering

The active turn inbox distinguishes preemptive events from completion events:

- user steering and Goal definition notices wake and cancel an in-flight Goal
  sampler so the request can be rebuilt;
- task completion does not interrupt a streaming reply and is drained at the
  next safe boundary;
- unsafe or side-effecting tools finish before either event is applied.

A steering-displaced wait receives one legal tool result saying the wait moved
to the background. The eventual payload is a hidden reminder, never a second
result for the original call id. Wait reservations belong to a turn, so pause,
cancel, shutdown, and background-task termination explicitly defer or consume
them before the worker is aborted.

## Goal scopes and leases

Every Goal stage and formal write carries a lease consisting of Goal id,
definition revision, autonomy generation, and stage id. Set, steering, pause,
clear, or a newer foreground generation invalidates old leases. Planner files,
verifier verdicts, Goal Plan updates, and completion decisions are proposals
until the actor validates the lease.

Goal-generated synthetic directives carry structured Goal metadata. Model
requests, verifier transcripts, and compaction inputs share the same projection
function:

- Active Goal autonomy includes matching context and autonomy directives;
- paused interaction includes matching context and paused directives;
- Normal and other Behaviors exclude every Goal directive;
- the completed receipt is never model context.

Todo/Plan state is internally scoped to Session or a Goal. ACP still receives a
full replacement `SessionUpdate::Plan`; scope is not added to the wire. Leaving
Goal republishes the Session plan, so Goal todo state cannot leak into Normal or
destroy a pre-Goal plan.

Planner and verifier workers write only lease-specific staging artifacts. The
actor validates and atomically publishes canonical artifacts. A paused,
revised, cleared, or completed Goal can therefore receive late diagnostics but
cannot receive late formal writes.

Staging retention: the planner writes definition-owned drafts under
`<session_dir>/goal/staging/`. Terminal Goal transitions (clear, verified
completion, explicit behavior exit) remove that transient directory; the
canonical `plan.md` and the immutable `plan.baseline.md` are retained as the
audit record of what the Goal executed against. An old revision's stale
staging draft is discarded at publish time when its lease no longer matches.

Session restart migrates only architecture_version-1 Goals. A persisted Goal
with an older `architecture_version` (other than a completed display-only
receipt) is not resumable under the current orchestration; it is dropped at
load and surfaced as a user-visible unified log entry so an upgrade restart
explains why the Goal vanished.

## Compaction

Manual and automatic compaction share one ownership lease. Inline automatic
compaction belongs to its running turn. Manual `/compact` is accepted through
the command plane and runs immediately when idle or at the next safe boundary,
ahead of queued prompts and Goal continuation. Conversation replacement never
runs concurrently with a turn or another compaction.

Compaction projects out inactive Goal directives before transcript extraction
or summarization. It cannot fold a cleared/completed Goal instruction back into
Normal history.

## Recovery

The durable `TurnCompleted` update is the terminal lifecycle authority.
PromptResponse carries ACP result metadata and is an idempotent second source.

A submitting prompt that receives no queue or terminal signal asks the shell
for that prompt's authoritative status. Time alone never fabricates a
terminal. The pager's watchdog polls `grow/queue/prompt_status` on bounded
schedules — every 2s while the prompt stays in Submitting, every 30s while it
stays in Running — and reconciles the displayed state with the shell's answer,
clearing the watch marker only when the authoritative status resolves it.
