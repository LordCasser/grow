## Why

The Windows session-storage workflow's pull request and `main` push filters omit `.cargo/**` and the workspace `Cargo.toml`. Changes to these direct Cargo build inputs can therefore miss the Windows regression job. The archived `audit-post-v2-4-0-boundaries` identified this as an early-feedback coverage gap, while confirming release `workflow_call` still invokes the workflow as a required gate.

## What Changes

- Add `.cargo/**` and root `Cargo.toml` to both push and pull request path filters.
- Keep the filter scoped to files that affect the Windows session-storage job; do not trigger it for arbitrary repository paths.
- Add regression checks for both trigger path sets and for the unconditional reusable release gate.

## Contract Impact

No product or archived specification behavior changes. The workflow path filters and existing release call contract are verified as repository CI configuration.
