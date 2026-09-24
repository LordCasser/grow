## Context

`find_session_match` resolves exact root and registered child IDs, then applies a race-window fallback for the active root when its `session_id` is still `None`. This is appropriate for benign startup updates that can precede `TaskResult::SessionCreated`. Permission requests carry authority-sensitive raw session identity, so an arbitrary unmatched ID must not inherit that fallback.

## Decision

Give permission admission a strict lookup that recognizes only an exact root session ID or an existing child-view key. Keep the general notification lookup and its startup fallback unchanged. `handle_permission_request` uses the strict lookup and cancels when no exact owner exists.

## Risks / Trade-offs

A permission request arriving before its session identity is registered will be cancelled. The routing contract already depends on session identity; accepting an unbound request would let a stranger target an active session, so strict admission is the safe boundary.
