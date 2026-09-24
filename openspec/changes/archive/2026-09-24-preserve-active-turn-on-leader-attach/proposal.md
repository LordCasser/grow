# Change: Preserve an active turn when a leader viewer attaches

## Why

The two multi-client PTY tests attach viewers as soon as the first streamed answer appears. A visible token is not a completed turn. A resident `session/load` then invokes subagent reconciliation, which unconditionally calls `recover_interrupted_durably` on the live parent's ChatState actor. This appends a Recovery terminal for the still-running turn. Its real terminal then fails with `TurnMismatch { active: None }`, and the next prompt cannot complete.

## What Changes

- Restrict post-subagent interrupted-scope recovery to a newly claimed writer incarnation; resident reconnect may reconcile completed child facts but must not close live parent work.
- Keep the existing completed-turn replay scenarios synchronized on the durable terminal, and add a paced running-turn attach regression.
- Include the multi-client cases in the selected PR regression workflow.

## Impact

Leader resident reconnect behavior, session Timeline ownership, and PTY coverage. No new persistence entity or compatibility branch.
