# Separate task-cancel entry scopes

## Why

`KillTerminalCommandTool` delegates to the mixed `KillTaskTool`, so a terminal-only entry can reach child cancellation after a terminal miss. The mixed entry also requires a terminal before it can cancel an authorized child, excluding hosts that provide only `SubagentBackendResource`.

## What Changes

Keep the existing tool names, shared `KillTaskAction` kind and mixed `kill_task` behavior. Route both entries through one cancellation implementation with an explicit scope: terminal-only cancellation never accesses the child backend; mixed cancellation prefers a matching terminal task and uses the session-bound child backend only when the terminal is absent or reports NotFound. Preserve backend authorization, idempotent outcomes and the existing result/receipt path.
