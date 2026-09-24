## Why

The ACP reverse MCP cancellation item records a deliberately unsupported transport capability, not an unhandled requirement. The archived `Abandoned MCP calls notify their originating service` contract explicitly limits the guarantee to stdio and Streamable HTTP and includes an ACP scenario that requires Grow not to claim remote cancellation delivery.

## What Changes

- Remove the ACP reverse bridge cancellation-notification item from the active backlog.
- Keep the accepted half-duplex ACP limitation documented in the canonical spec and developer guide.

## Capabilities

No behavior or contract changes. `.openspec.yaml` sets `skip_specs: true`; this closes an optional transport expansion that is explicitly outside the current contract and adds no spec delta.

## Impact

- `openspec/backlog.md` only; no code or developer guide changes.
