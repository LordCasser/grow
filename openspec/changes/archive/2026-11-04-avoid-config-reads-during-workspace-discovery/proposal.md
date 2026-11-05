## Why

Entering a session in a Synology checkout took 78 seconds; Agent selection and `grow inspect` also stalled. Process sampling and an independent read identified `.git/config` as a dataless placeholder whose read returns OS error 60. Root-only callers use `git2::Repository::discover`, which discovers a path and then opens the repository, unnecessarily reading that config repeatedly.

## What Changes

- Discover checkout boundaries using libgit2's path-only discovery, shared by existing Agent/configuration and session consumers.
- Preserve normal, linked/separate worktree, symlink, bare-repository and home-directory boundaries.
- Resolve trust's existing filesystem identities from Git topology without opening Git configuration; missing or invalid required topology still fails closed.
- Leave Git content operations (status, commits, remotes and worktree management) on their existing APIs.

## Impact

Affected capabilities: configuration-rules and client-surfaces. No user repository files, persisted session format, new cache, retry or timeout policy changes.
