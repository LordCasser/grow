# Input routing

Grow routes interactive input by intent, not by the current behavior mode.
Goal, plan, and future behaviors therefore share the same admission rules.

## Invariants

1. Plain input submitted with Enter is a user prompt. The shell owns its FIFO
   queue, so input remains accepted while another turn is running.
2. Send now is an interjection. It enters the active model loop at the next
   safe boundary; if the turn ends first, the shell converts it to the next
   queued prompt instead of dropping it.
3. Leading-slash input is a Grow command. Known host commands use the
   `grow/commands/execute` control plane while a turn or goal is active. They
   never enter the model prompt queue.
4. Unknown leading-slash input is an error. It is never downgraded to a user
   prompt.
5. Skill and workflow commands that intentionally start model work may be
   queued as expanded work, but the raw slash invocation is still resolved by
   Grow before model admission.

## Ownership

The pager classifies input and chooses the transport. The session actor is the
authority for queue ordering, interjection fallback, command execution, goal
state, and turn cancellation. Control commands share the actor mailbox with
prompt admission, which serializes state changes without blocking on the model
task.

Goal controls that only inspect or update state (`status`, `budget`) execute in
place. `pause` and `clear` persist their state transition before cancelling the
running turn. Any interjections not yet consumed are first converted back into
queued prompts, preserving user input across the cancellation. Replacing an
active goal cancels work on the old objective and promotes the new goal
reminder as the next turn.
