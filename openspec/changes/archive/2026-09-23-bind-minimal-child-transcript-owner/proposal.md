# Bind Minimal transcript builds to the selected child

## Why

In Minimal mode, `/transcript` snapshots the top-level Agent even when the user is editing an open child view. The incremental pump, pager handoff, and failure notices then use the parent as owner. The resulting pager can show a different conversation from the one that accepted the command.

## What Changes

Capture the exact root or child view and its session identity at request time. Keep that owner through frame slices and view switches, restart from the same view after its reload, and abandon a build whose view was removed or rebound. This change addresses the external transcript command; the separate Minimal native scrollback rendering mismatch remains tracked in the backlog.
