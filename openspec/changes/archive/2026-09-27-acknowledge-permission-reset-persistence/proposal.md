## Why

`ResetState` currently acknowledges after clearing memory, while the root permission file write runs in another task and silently logs failures. A crash before that write can restore old remembered grants; a write failure is invisible to the user.

## What Changes

Complete Reset only after the serialized latest-state write returns. Propagate persistence errors through the permission handle and publish a Shell UI-only error notice for reset notifications. Keep the manager mailbox free of filesystem waits; request processing and scoped cancellation remain independent.

## Capabilities

### Modified Capabilities

- `tool-authorization`: distinguish immediate in-memory revocation from durable Reset completion and expose write failure.
- `client-surfaces`: show reset persistence failure as a UI-only notice, not assistant text.

## Impact

`PermissionHandle::reset_state` returns `Result`. Normal reset completion waits for its durable file operation; failed storage leaves the current in-memory reset in force and explicitly reports that restart persistence is unconfirmed.
