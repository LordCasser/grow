## Why
The full Shell suite exposed a reaper stop notification timeout, while the same test passes alone. The process-wide reaper blocks on each received JoinHandle, so an earlier still-running session prevents cleanup of later completed sessions and their persistence mailboxes.

## What Changes
Keep unfinished jobs pending in the existing single reaper thread and join only completed handles. Preserve stop-after-join ordering, weak persistence routes and final-owner semantics. Add a deterministic blocked-predecessor regression rather than widening the timeout.
