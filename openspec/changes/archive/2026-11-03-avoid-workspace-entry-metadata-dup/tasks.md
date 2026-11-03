## Implementation and verification

- [x] Confirm workspace listing baseline and its existing ordering, confinement, link, and pagination tests.
- [x] Reuse ordinary-entry metadata from `symlink_metadata`; continue fetching target metadata for symlinks.
- [x] Exercise the client-facing listing operation for regular files/directories, symlink target metadata, dangling links, root confinement, pagination, and cap behavior.
- [x] Record comparable large-directory before/after measurements and the unmeasured symlink-heavy case.
- [x] Run targeted workspace checks and `openspec validate --all --strict --no-interactive`.
