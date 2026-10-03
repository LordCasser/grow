## Why

Grow 2.3.0 is the current published version. The completed, archived changes since that release improve read-only replay surfaces and restore Timeline-backed communication in its original position during resume. The workspace version, release notes, and release tag need one consistent identity before publication.

## What Changes

- Prepare patch version 2.3.1 in workspace metadata and the lock file.
- Summarize the replay and ordered resume-history improvements in public release notes.
- Validate the committed source, version metadata, OpenSpec archive, and clean tagged release preflight before publishing.

No product contract changes in this release-preparation change. `skip_specs: true` is used because the behavior is already covered by its archived OpenSpec changes. Cross-platform builds and publication remain the responsibility of the official release workflow.
