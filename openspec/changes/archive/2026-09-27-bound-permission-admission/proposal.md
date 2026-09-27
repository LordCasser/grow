## Why

Request-local waits and parallel Sidebands removed head-of-line blocking, but both dispatch channels are unbounded and each received item can create an independent task. A burst of child permission requests can retain many request bodies, parent context snapshots and provider connections until deadline.

## What Changes

Add fail-fast finite admission for the shared permission manager and for independent permission Sideband tasks. Accepted requests remain concurrent. Saturation does not enqueue an external model wait behind another judgment; it returns an explicit non-authorizing result and increments a bounded-label metric. Cancellation releases capacity.

## Capabilities

### Modified Capabilities

- `tool-authorization`: define bounded admission and overload behavior.

## Impact

Overload is visible to requesting Agents and metrics. Normal requests retain current policy, deadlines and Sideband concurrency.
