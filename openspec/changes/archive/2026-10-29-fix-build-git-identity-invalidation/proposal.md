## Why

Pager and CLI build scripts watch `.git/HEAD` relative to their package directories. Those paths do not exist, so Cargo notices the missing path on each build and can rebuild repeatedly. The scripts also need to track the actual Git identity inputs so a branch ref update invalidates the embedded commit in both normal checkouts and linked worktrees.

## What Changes

- Resolve Git metadata from each crate's manifest directory and emit Cargo rerun paths for the worktree `HEAD`, common `packed-refs`, and the current symbolic branch ref.
- Preserve the existing Pager and CLI version/commit selection, including CLI `RELEASE_COMMIT` precedence.
- Verify path discovery with temporary real Git repositories, including a linked worktree, a packed ref, and detached HEAD.

## Scope

This is build invalidation maintenance. It does not change the accepted runtime contract or user-visible version format, so `skip_specs: true` is intentional and no delta spec is added.
