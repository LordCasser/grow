# Portable transcript tree export

## Problem

CLI transcript tree creation writes and syncs each file, then opens each containing directory as a file and syncs it. Windows does not support opening directories through `std::fs::File`, so this final directory-sync pass rejects an otherwise complete export. Trajectory archive creation uses the same tree writer and fails for the same reason.

## Change

Keep per-file write and sync failures fatal. Sync directory entries on Unix, where the existing standard-library approach is supported; skip only the unsupported directory-handle operation on non-Unix platforms. Sync the CLI destination parent after the no-replace rename so the published name follows the same durability boundary. Continue propagating every error from operations that are attempted, including file sync, tree generation, and publication-parent sync.

## Scope

This change fixes the platform boundary and adds regression coverage for real parent/child transcript generation, a multibyte session path, archive extraction, and HTTP streaming cleanup when a response completes or its client abandons it. It does not change transcript contents, source capture, budgets, or publication rules.
