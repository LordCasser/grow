# Change: Verify nonaggregated sampling evidence

## Why

The backlog still questions whether partial SSE, parser failures, empty responses, retry decisions and interrupted attempts retain causal evidence. The existing request/attempt evidence pipeline already defines these boundaries, but the remaining claim needs direct fault-injection verification before it can be closed.

## What Changes

- Exercise partial and malformed provider streams on all three backends with raw-response and terminal-decision assertions.
- Verify that a recovered open attempt remains unconfirmed and cannot become accepted output.
- Remove the backlog item only if these checks confirm the existing behavior. Record any real defect separately rather than weakening the assertion.

## Impact

Verification and backlog documentation only. This does not change accepted behavior or a contract, so `skip_specs: true` applies.
