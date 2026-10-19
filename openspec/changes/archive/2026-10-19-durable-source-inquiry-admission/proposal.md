## Why

`SessionCoordinationBackend::ask_with_id` writes `OutgoingStarted` through the source SessionActor before dispatching the inquiry, but the actor currently appends that fact on every call. A retry after an unknown command acknowledgement can therefore duplicate the startup audit and its UI projection even though the canonical Timeline already accepted the operation.

## What Changes

- Make the source SessionActor the serialized durable admission owner for `OutgoingStarted`.
- Treat a retry with the same source session, inquiry ID, target, and question as already admitted without another Timeline append or UI projection.
- Return `Conflict` for an existing inquiry ID bound to a different target or question, without writing a new fact or dispatching it.
- Resolve an uncertain acknowledgement by checking canonical Timeline facts before accepting a retry; do not add a second inquiry store or deduplicate by question text.

## Capabilities

### Modified Capabilities

- `local-coordination`: specify idempotent source-side startup admission and conflict behavior.

## Impact

Changes are limited to the existing coordination Timeline command, its SessionActor owner, and focused durable admission tests. Target-side execution, inquiry identity, and runtime transport are unchanged.
