## Why
TTL cleanup still retains one OpenedSession directory per discovered session before processing candidates. Large histories can exhaust descriptors despite sequential deletion. Retained candidates after a failed/refreshed recheck can also retain maintenance writer leases until the whole adapter drops.

## What Changes
Discover summaries without retaining per-session handles, then establish current eligibility one candidate at a time. Keep full discovery before deletion, same-entity rechecks under writer ownership, skip paths and deletion callbacks. Bound each candidate's maintenance cache/lease lifetime to that operation.
