## Why

The backlog describes reconnect-time recovery of an in-flight sampling preview as conditional work. The archived contract and implementation intentionally discard an unconfirmed preview after reconnect and reconstruct only admitted history. This entry therefore tracks an optional continuity experience rather than an unmet contract or a demonstrated user fault.

## What Changes

- Remove the conditional active-candidate-prefix replay entry from `openspec/backlog.md`.
- Narrow the local-draft invalidation restart entry to the concrete low-frequency failure window: deletion keeps failing until Pager exits, so its in-memory retry intent is lost and the old draft can return on the next startup.
- Keep the ACP reverse MCP bridge cancellation entry, whose transport limitation remains a distinct behavior gap.
- Do not change code, product behavior, or archived specifications.

## Impact

Documentation only. `.openspec.yaml` sets `skip_specs: true` because no behavior contract changes; the existing reconnect contract remains authoritative.
