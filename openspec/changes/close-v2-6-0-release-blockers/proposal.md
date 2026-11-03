## Why

The v2.6.0 candidate has eight independent verification-debt entries, including two reproduced release-gate failures. Their behavior fixes and validation belong to separately scoped changes; a single release-level record is needed to track evidence, distinguish closed work from explicit deferrals, and keep the final release gate tied to the candidate source.

## What Changes

- Preserve the eight-item non-deferred baseline from `openspec/backlog.md` at this change's start.
- Track each item to its owning behavior/test change, required regression evidence, and final disposition.
- Record the release gate commands and the exact-source, tag, workflow, and asset evidence required after all blockers close.

This change coordinates evidence only. It uses `skip_specs: true` and introduces no product behavior or delta spec. Each behavior fix must use its own change and update the applicable contract there. The nine explicitly deferred backlog items remain outside the eight-item non-deferred baseline and are retained in the active backlog.
