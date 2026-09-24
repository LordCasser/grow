## Why

Subagent permission classification needs request details, but completion audit events currently retain those details and the live Pager projection also exposes unbounded request text and classifier prose. A truncated classifier request can still be judged as though it were complete, so the boundary must fail closed when the full decision detail exceeds its budget.

## What Changes

- Bound classifier decision details and return Unavailable without inference when the full request exceeds its applicable budget.
- Remove raw access details and classifier prose from completed permission audit events; project bounded safe summaries identically to live Pager and durable replay.
- Keep active permission decision inputs available only to the policy/classifier path that needs them.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- tool-authorization: bounded classifier inputs and post-decision evidence lifetime.
- client-surfaces: bounded, redacted subagent permission audit projection for live and replayed UI.

## Impact

Workspace permission classification and event types, Shell permission audit projection and notification schema comments, Pager permission audit display state, and focused regression tests. No dependency changes.
