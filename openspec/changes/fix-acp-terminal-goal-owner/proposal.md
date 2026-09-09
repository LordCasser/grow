## Why
The ACP terminal adapter retains goal_id but discards goal_definition_revision from TerminalRunRequest. Its task snapshots and completion notifications consequently contain a partial Goal owner and are rejected by the notification bridge. Adjacent Goal owner propagation is audited in audit.md.

## What Changes
Retain the supplied immutable revision alongside the Goal id in tracked ACP background tasks and every derived snapshot. Strengthen the existing adapter integration regression to verify both fields in lookup and completion.
