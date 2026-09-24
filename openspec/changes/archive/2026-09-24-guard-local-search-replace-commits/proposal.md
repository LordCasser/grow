# Change: Guard local search-replace commits against stale reads

## Why

`search_replace` computes a complete replacement from a file read, then calls unconditional `write_file`. Two child processes can both read the same bytes and overwrite one another's edits. The empty-file update path has the same gap. The existing exclusive-create operation protects only a target that was absent.

## What Changes

- Add a conditional existing-file write to the tool filesystem boundary. Local filesystem writes compare the exact bytes observed by the tool while holding an advisory lock on the opened file, and fail on mismatch before changing it.
- Use the conditional write for ordinary replacement and existing empty-file updates; retain atomic no-replace creation for missing targets.
- Fail closed when a filesystem adapter cannot provide the conditional operation. A failed condition emits no `FileWritten` notification.

## Capabilities

### Modified Capabilities

- `tool-authorization`: local edit commit behavior and conflict response.

## Impact

The protection serializes Grow writers that use this edit path and detects non-cooperating writes that finish before the conditional comparison. POSIX advisory locking does not make arbitrary external writers participate, so the contract does not claim an atomic compare-and-swap against a simultaneous non-cooperating write. ACP has no conditional write operation and rejects existing-file edits rather than silently overwriting newer bytes.
