# Observe stdio session close before replay

## Why

The stdio bridge forwards session close requests to the leader while its outgoing replay-cache filter only admits initialize/new/load. The existing cache helper's close branch is unreachable from the real forward path, so a later reconnect reloads closed sessions (PB-01).

## What Changes

Route every nonempty forwarded JSON message through the existing cache parser, which already selects the lifecycle methods it owns. Preserve its existing close semantics and session replay order. Verify forwarding through reconnect with two sessions and both currently supported close spellings. Concurrent session/new identity matching remains PB-02 and is not changed here.
