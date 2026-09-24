# Change: Verify the scrollback layout precondition

## Why

The Pager backlog records a possible render panic from `ScrollbackPane` reading a missing layout cache or an out-of-bounds paint range. The record has no concrete production trigger. Keep or remove it based on the actual render call path and cache/range invariants.

## What Changes

- Trace every production `ScrollbackPane` render and the state mutations between layout preparation and rendering.
- Trace cache sizing, visible range maintenance, and paint-window bounds; run existing layout regressions.
- Remove the speculative backlog item only if the production path satisfies the precondition.

## Impact

Audit and backlog documentation only. This changes no runtime contract, so `skip_specs: true` is set.
