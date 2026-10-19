## Why

Search cache self-healing currently increments a process-global epoch. When one storage root's index is quarantined or recreated, concurrent searches and bootstraps for other roots observe the same epoch change and treat their own cache as replaced.

## What Changes

- Scope recovery epoch changes to the effective search-index path.
- Bind query and bootstrap completion checks to that same index identity.
- Verify that healing one root does not trigger another root's rebootstrap or invalidate its completion marker.

## Capabilities

### Modified Capabilities
- `client-surfaces`: cache replacement and bootstrap invalidation are scoped to the affected search index.

## Impact

Only search-cache recovery epoch tracking and its callers/tests change. Search schema, lease format, and cross-process coordination remain unchanged.
