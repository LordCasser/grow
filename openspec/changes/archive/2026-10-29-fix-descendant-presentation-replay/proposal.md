## Why

Descendant restoration dispatches a child log's typed lifecycle presentation before confirming that its parent, subagent and child identities match the validated canonical spawn. A mismatched presentation can therefore create a task row or child projection. Replaying the same valid legacy Spawned presentation without an eventId also appends another Started row on every forced restoration.

## What Changes

- Project descendant lifecycle updates only when their identities match the canonical parent spawn and validated child relationship for the source session.
- Reuse an existing Started row for the same child when an ID-less Spawned update is forced through restoration again.
- Keep these checks and projections presentation-only; they do not grant child execution or resume authority.

## Capabilities

### Modified Capabilities

- `client-surfaces`: descendant presentation recovery validates cross-session ownership and remains idempotent without optional event IDs.

## Impact

Pager descendant restore/replay and the existing validated storage relation reader. No execution or export behavior changes.
