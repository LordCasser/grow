## Why

Grow 2.2.2 is the current published version. The completed, archived work since that release includes session transcript tree export, read-only replay, and a set of TUI, sampling, and workflow fixes. The workspace version, release notes, and eventual annotated tag need one consistent release identity before publication.

## What Changes

- Prepare the next minor version, 2.3.0, in workspace metadata and the lock file.
- Write a public changelog entry that calls out the changed `grow export` CLI contract and summarizes the completed features and fixes.
- Verify the feature commit, version metadata, OpenSpec archive, and local release preflight; create an annotated local tag only after the final release-preparation commit is clean.

No product contract changes in this release-preparation change. `skip_specs: true` is used because the behavior changes are already in their own archived OpenSpec changes. Publishing and the cross-platform release workflow remain separate operations.
