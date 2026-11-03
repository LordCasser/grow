# Design

## Current ownership

`finish_child` removes an execution from `active`/`pending`, inserts its completion data in `retiring` when a physical owner remains, and may also insert the same identity into `finalizations` when terminal persistence is pending. The recovery tick advances finalizations and sweeps retirement. The current spawn check sums pending, active, and finalizations, so it misses retirement-only identities and double ownership is not yet represented in one shared count.

## Decision

At spawn admission, sweep already-retired physical owners, then count the unique identity union of `pending`, `active`, `finalizations`, and `retiring` against `MAX_FINALIZATIONS`. Sweeping before admission allows a completed join to release its slot without waiting for another timer tick. The same `retiring` record can remain alongside a pending finalization; set-based identity accounting deduplicates it.

No owner is removed unless `suspension_retired` returns `Ok(true)`. Errors and unfinished owners continue to occupy capacity and remain available for the existing sweep/recovery path.

Spawn identity uniqueness also checks `retiring` and `finalizations` directly. The completed map is a bounded presentation cache, so it cannot be the only duplicate guard for a still-owned physical retirement or finalization responsibility.
