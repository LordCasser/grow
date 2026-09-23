## Why

The memory crate's top-level layout example still describes a bare hash of the
workspace path. `MemoryStorage::new_inner` actually derives a readable
`{slug}-{hash8}` directory, preferring the repository remote identity and
falling back to the canonical workspace path. The example should match the
implementation and its path-shape tests.

The backlog also retains a completed HTTP Hook boundary entry whose three
verification records are already archived and have no remaining item-specific
work.

## What Changes

- Correct the `crates/codegen/memory/src/lib.rs` data-layout comments to state
  the repository-identity/path-fallback directory shape.
- Remove the resolved Memory comment-drift and HTTP Hook completed entries from
  `openspec/backlog.md`.
- Do not change behavior or specifications; this is a documentation and
  backlog-maintenance change, so `skip_specs: true` is intentional.

## Impact

Only developer-facing comments and resolved backlog bookkeeping change. The
existing implementation and path-shape tests remain the evidence for the
documented layout.
