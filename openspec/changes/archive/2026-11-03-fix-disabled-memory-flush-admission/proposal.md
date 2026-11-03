# Prevent memory flush work after memory is disabled

## Why

Turning memory off during a session removes its storage and tools, but the pre-compaction flush configuration remains active. A later compaction can therefore send a memory-flush request after the user disabled memory. A flush already waiting for image projection can also outlive the toggle.

## What Changes

- Make pre-compaction flush admission and provider dispatch observe the session's current memory-enabled state.
- Stop a flush before writing if memory was disabled while its provider request was in flight.
- Preserve frozen snapshot and image projection behavior; this change only gates flush work against runtime memory state.

## Impact

- Affected capability: `memory-search`.
- Affected implementation: Shell session actor memory flush and pre-compaction admission.
