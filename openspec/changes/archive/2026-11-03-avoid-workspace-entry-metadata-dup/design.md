# Design

`workspace::file_system::walk_fs_entries` currently probes each entry with `symlink_metadata`, then unconditionally follows it with `metadata`. The listing uses the second result for type, size, and modified time, and the first only for `is_symlink`.

Use the `symlink_metadata` result for ordinary files and directories. When it identifies a symlink, retain the existing `metadata` follow operation and skip the entry if its target cannot be read. If the initial `symlink_metadata` fails, retain the current behavior of attempting `metadata` and treating the symlink marker as false; this keeps the existing race/error fallback intact. Do not change `symlink_stays_in_root`, walk ordering, sort keys, cap handling, or paging.

Verification uses the existing workspace listing path for regular files/directories, symlinks to files/directories, dangling symlinks, escaping symlinks, stable sorted pages, and collection cap. A large-directory probe on the same listing API compares the baseline and changed implementation; report that it measures ordinary-entry metadata on this host and does not measure symlink-heavy workloads.
