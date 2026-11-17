## Why

When a second ACP client loads a session whose resident scheduler has fired since the historical creation event, replay restores the original scheduled-task row but cannot reconstruct the updated next fire time or most recently admitted child. The scheduler actor already owns both facts; the load projection should use that live authority.

## What Changes

Return a versioned snapshot of the resident scheduler with `session/load`, including each active task's next fire time and last admitted subagent identity. Pager adopts the snapshot for load and resync, while rejecting older scheduler notifications that race snapshot application.

## Scope

This change affects session-load response metadata and the Pager's scheduled-task projection. It does not change scheduling, firing, persistence, or child execution.
