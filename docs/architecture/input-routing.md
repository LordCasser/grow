# Input routing

Grow routes interactive input by intent, not by the current behavior mode.
Goal, plan, and future behaviors therefore share the same admission rules.

## Invariants

1. Plain input submitted with Enter is a user prompt. The shell owns its FIFO
   queue, so input remains accepted while another turn is running.
2. Send now during an active Goal is an interjection. For a previously queued
   row, the session actor atomically removes the authoritative queue item and
   moves its payload into the interjection buffer. It cannot both inject and
   later run as a standalone prompt. Outside Goal mode, Send now retains the
   normal cancel-and-promote behavior.
3. Leading-slash input is a Grow command. Goal commands always use the
   `grow/commands/execute` control plane, including while idle; other known host
   commands use it while a turn or Goal is active. The raw command is never a
   user message. Command acknowledgement is an agent response log.
4. Unknown leading-slash input is an error. It is never downgraded to a user
   prompt.
5. Skill and workflow commands that intentionally start model work may be
   queued as expanded work, but the raw slash invocation is still resolved by
   Grow before model admission.

## Ownership

The pager classifies input and chooses the transport. The session actor is the
authority for queue ordering, interjection fallback, command execution, goal
state, and turn cancellation. Control commands share the actor mailbox with
prompt admission, which serializes state changes without blocking on model
work. A control handler must never await a planner, sampler, subagent, or other
operation whose events return through the actor mailbox. Goal commands that
need model work are admitted as hidden `GoalControl` turns; their raw slash
text is resolved inside that turn and never enters user history or model input.
Pure state controls execute directly in the mailbox.

Behavior selection is one such pure control transition. `/goal set`, bare
`/goal`, and `/goal resume` first commit Goal Behavior and publish the
authoritative `CurrentModeUpdate`; only their planner/implementer work is
deferred. Selecting Goal from the Behavior picker with no existing Goal makes
the first ordinary text submission equivalent to `/goal set <text>`.
Hidden Goal control turns are durable queue entries: user-prompt priority may
discard replaceable completion wakes, but it must not sweep Goal controls,
scheduled fires, or Plan-resume turns.
When a Goal is paused, ordinary inputs remain admitted but are not promoted;
an explicit Goal control may pass the lifecycle barrier to resume or clear it.
Leaving Goal Behavior also releases the normal queue drain.

Pause, budget limits, definition edits, and automatic back-off retain Goal
Behavior. Only `/goal clear` and verifier-confirmed completion automatically
return to Normal. User messages queued under Goal are retagged to the target
Behavior when Goal is intentionally exited, so stale prompt metadata cannot
silently re-enter a cleared or completed Goal.

Goal definition, lifecycle, and resource constraints are orthogonal:

- `set` revises a non-terminal Goal definition in place. It preserves Goal
  identity, execution state, elapsed/token accounting, and the existing budget
  unless a new budget is explicit. It increments a monotonic definition
  revision; planner/evaluator/verifier/strategist results captured under an old
  revision are discarded at commit. If the definition changes while initial
  planning is in flight, the hidden Goal turn replans the latest revision
  before implementer inference starts.
- `budget` changes only the resource constraint and never cancels a turn.
- `pause` is the sole Goal command that cancels the running turn. Pending user
  interjections are converted back to queued prompts before cancellation. The
  same preservation applies to TUI cancellation and a confirmed switch away
  from an active Goal.
- `resume` changes lifecycle state and schedules a hidden system reminder; it
  is never represented as user input.
- `clear` is rejected while a Goal is active, so it cannot become an implicit
  second cancellation command.

Hidden Goal reminders and skill announcements share the session's buffered
system-reminder channel and drain at model-safe boundaries. User corrections
use the distinct interjection channel and remain real user messages.
