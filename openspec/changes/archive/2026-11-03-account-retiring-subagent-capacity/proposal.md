## Why

Coordinator capacity currently counts pending children, active children, and pending finalizations. A child can leave `active` while its physical runtime owner remains in `retiring`, allowing repeated launches to exceed the intended recovery and runtime retention bound.

## What Changes

- Count still-owned retiring child identities against the existing subagent capacity limit.
- Count identities once when the same child is present in both `retiring` and `finalizations`.
- Keep retirement and finalization ownership until their existing completion conditions are confirmed.

## Scope

This changes in-memory coordinator admission behavior only. It adds no timeout, eviction, or new capacity setting.
