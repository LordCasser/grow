## Why

An explicit `GROW_SCROLL_LOG` target is opened with create/write and truncated on the first record. Two recorders can therefore truncate or overwrite one another's output. The target needs process-level writer ownership before any destructive operation.

## What Changes

Explicit scroll log targets acquire a nonblocking exclusive lock on the opened regular file before truncating it. A recorder that cannot acquire ownership disables itself without changing the file. The owner keeps the lock for the recording lifetime.

## Capabilities

### Modified Capabilities

- client-surfaces: ownership of explicit scroll diagnostic paths.

## Impact

Only the scroll flight recorder's explicit file open and its regression coverage change. Generated default paths remain independent; special-file rejection and the per-recorder byte limit remain in effect.
