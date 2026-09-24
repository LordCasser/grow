## Why

Pager permission selection trusts the dispatched option ID before popping the queued request. A malformed or stale action can therefore trigger the special session-wide Always Approve side effect even when that ID was never offered for the front request, including a child request whose persistent options were removed.

## What Changes

- Require a permission selection to match an option offered by the exact front request before mutating the queue, replying, or changing permission mode.
- Leave the queued request and session state unchanged when the selection is invalid.

## Capabilities

### Modified Capabilities

- `client-surfaces`: permission selection is validated against the queued request before any side effect.

## Impact

Pager permission dispatch and focused regression coverage. No permission protocol or workspace authorization changes.
