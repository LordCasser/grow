# Apply busy effort changes to the active Pager view

## Why

When `/effort` changes a running session from high to max, Shell applies the change and emits a success receipt, but Pager can keep showing high. Pager parks `ModelChanged` while the local Sampling intent is in flight. The matching terminal `ControlStateUpdate` clears the intent without releasing that parked model projection. A later max selection is then an actual no-op in Shell and correctly has no success notice, which makes the picker appear broken. This diverges from the current client-surfaces requirement that a real effort change remains visible.

## What Changes

- Apply the parked authoritative model projection when its matching control terminal resolves the local intent, including while the foreground is busy.
- Keep the existing Shell-owned step boundary, terminal notice, and no-op notice rules.
- Cover the notification order and a concurrent control in another domain with a Pager regression test.

## Scope

Pager control notification projection only. No change to Shell admission, model catalog, picker navigation, persistence, or control receipt format.
