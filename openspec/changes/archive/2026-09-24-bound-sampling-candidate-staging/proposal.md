# Bound sampling candidate staging

## Why
Session persistence currently retains every tagged candidate notification payload during an attempt, although accepted canonical content is supplied by `ResponseReplayProjection`. Fragmented previews therefore add unbounded duplicate text to staging memory.

## What changes
Keep only whether/where the first candidate occurred, and retain untagged notifications in order. Tagged candidate payloads are never retained or persisted. Projection commit inserts its canonical response at the first-candidate boundary; discard and shutdown continue to omit candidates while preserving untagged events.

## Impact
This changes only transient persistence staging. Durable timeline facts and canonical replay behavior remain governed by the existing projection contract.
