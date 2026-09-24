## Why

Pager coalesces incoming passive inquiry notices into one row keyed by `(source_peer_id, inquiry_id)`. The scrollback upsert currently panics if given a block without coordination identity, and an empty correlation ID can pass the notice audit parser, creating an ambiguous row key.

## What Changes

- Require a non-empty source peer ID and inquiry ID for passive inquiry row coalescing.
- Preserve incoming notices with missing or invalid identity as finite raw notices, without entering passive running lifecycle.
- Make the lower-level scrollback upsert reject absent or empty identity without panic or mutation.
- Keep current passive lifecycle projection and its foreground-cleanup protections unchanged; its generic running fields remain a separately tracked design boundary.

## Capabilities

### Modified Capabilities

- `client-surfaces`: define fail-closed handling for malformed passive inquiry identity.

## Impact

- Implementation: Pager session-notification projection and coordination scrollback state.
- Tests: missing/empty identity rejection, raw notice preservation, and existing passive lifecycle behavior.
- No change to local coordination protocol or durable inquiry identity.
