# Change: Suppress automatic recaps during reconnect

## Why

Pager keeps the previous shell's recap capability while reconnect initialization and session reload are in progress. The away-recap poll and focus-return path can therefore dispatch `grow/recap` to a replacement endpoint before its sessions are loaded or its recap capability has been applied. Automatic failures are silent, and a focus-return attempt can consume the away period without producing a recap.

## What Changes

Suppress automatic recap eligibility and dispatch while `reconnect_pending` is true. Polling during reconnect remains a no-op without recording an attempt. Focus return during reconnect drops that best-effort opportunity as the existing focus transition clears the away period. Reconnect completion continues to publish the replacement shell's capability through the existing gate.

## Capabilities

### Modified Capabilities
- client-surfaces: automatic recap admission during reconnect.

## Impact

Pager recap eligibility and dispatch only. No capability re-advertisement protocol or deferred recap intent is introduced.
