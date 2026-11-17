# Fix child TODO projections

## Why

Pager routes live Plan updates and deferred child transcript replay through `AgentSession::handle_update`, which intentionally does not update a session's Todo pane. A child therefore loses its local TODO projection even though its committed Plan is present in ACP history.

## What changes

Project Plan updates into the owning child view's Todo pane both when updates arrive live and when inherited child history is replayed. Keep root TODO routing unchanged and derive entries through the existing plan-to-Todo conversion.
