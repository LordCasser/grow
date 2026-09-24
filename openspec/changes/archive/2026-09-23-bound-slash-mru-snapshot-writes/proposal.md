## Why

Slash MRU loading rejects files larger than 1 MiB and disables persistence for that store, but its writer can currently publish a larger snapshot. The writer should enforce the same encoded-byte limit so it never creates a file that its own loader will reject.

## What Changes

- Reject encoded MRU snapshots larger than 1,048,576 bytes before background handoff and before filesystem publication.
- Preserve the prior destination on rejection and return handoff failure so the controller keeps the store dirty.
- Keep valid snapshots at or below the existing read limit on the current atomic publication path.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `client-surfaces`: define the encoded-byte limit and failure behavior for MRU snapshot writes.

## Impact

Changes `crates/codegen/pager/src/slash/mru.rs`, tests of the real `SlashController::record_command_use` path, and the Slash MRU guidance in `docs/development.md`. The change adds no dependency or public API.
