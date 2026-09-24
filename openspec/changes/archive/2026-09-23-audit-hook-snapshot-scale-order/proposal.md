# Audit Hook recovery snapshot scale and ordering

## Why

`PublishControlState` republishes completed Hook projections after session creation/load. The backlog asks whether scanning and copying the full completed history is costly at large history sizes, and whether its order can be combined with replayed UI updates.

## What Changes

- Trace the Timeline query, projection, serialization, and client presentation paths.
- Measure synthetic history scaling when the focused harness can run.
- Separate completion order within Timeline from cross-ledger UI ordering.
- Keep or narrow the backlog item based on evidence; do not alter production behavior in this audit.

## Impact

Audit and developer guidance only. No runtime behavior or archived contract changes; `skip_specs: true` is appropriate.
