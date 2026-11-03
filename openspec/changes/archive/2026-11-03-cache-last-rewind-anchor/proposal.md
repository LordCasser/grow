## Why

`prompt_cache_key` currently scans Timeline events backward on every request to find the latest Rewind event. The Timeline already folds accepted events incrementally, so it can retain this derived sequence alongside its other projections.

## What Changes

This change adds a private `Option<EventSeq>` projection updated only when a validated Rewind event is applied. Request key generation reads that value instead of rescanning the event vector. The event log remains authoritative and the generated key bytes and rewind semantics remain unchanged.

## Impact

This is an internal performance refactor. It does not change a public or persisted contract, so this change uses `skip_specs: true`.
