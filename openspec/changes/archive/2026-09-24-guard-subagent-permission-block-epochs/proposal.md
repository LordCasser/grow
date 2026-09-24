## Why

Pager permission rows are grouped by primary-turn epoch, but `SubagentPermissionBlock` exposes unrestricted mutation methods that can append to any existing group. Restricting mutation to the scrollback state owner makes the existing epoch checks the single path for keeping audit rows within their turn boundary.

## What Changes

- Make permission block membership mutation crate-internal and require the source epoch when appending or merging members.
- Reject additions whose source epoch differs from the block epoch; preserve the existing state-level grouping and reconnect behavior.

## Capabilities

### New Capabilities

### Modified Capabilities
- `client-surfaces`: permission audit groups retain their primary-turn epoch boundary during append and reconnect merge.

## Impact

Affected code is Pager's `SubagentPermissionBlock` and `ScrollbackState` permission-group append/reconnect paths. No permission decision or durable audit payload changes.
