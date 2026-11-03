## Why

Workspace directory listings call both `symlink_metadata` and `metadata` for ordinary entries, although the first result already contains the regular file or directory metadata used by the listing. Avoiding the duplicate syscall reduces work for ordinary entries in large listings.

## What Changes

Reuse the first metadata result for non-symlink entries. Continue following symlinks to obtain target metadata, preserve the symlink marker, and retain current listing, confinement, pagination, and failure behavior.

## Scope

This is an internal syscall reduction with no intended observable behavior or contract change. `skip_specs: true`; no delta specification is added.
