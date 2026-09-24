## Context

`dispatch_permission_select` currently pops the front `PermissionViewState` before it checks whether the supplied ID is present in that request's options. The global Always Approve effect is keyed directly from the supplied ID, while child requests normally omit persistent options. This allows a stale or malformed action to reach a side effect despite not being a valid answer to the request.

## Decision

Peek at the front request and verify its option list contains the supplied ID before computing or applying selection side effects. If no front request exists or the ID is absent, return without changing state. After validation, keep the existing pop, metadata construction, response, queue transition, and mode behavior.

This makes the queued request's own option list the authority and adds no new state or protocol field.

## Risks / Trade-offs

Invalid internal actions become no-ops and leave the prompt available for a valid answer. No user-visible toast is added because the invalid action does not resolve the pending request and may be a stale UI event.
