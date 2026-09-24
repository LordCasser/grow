## 1. Bind quarantine to its source

- [x] 1.1 Pass the opened source handle to quarantine and leave a mismatched current entry untouched. Verified by the deterministic opened-corrupt-source/path-replacement regression test.
- [x] 1.2 Preserve regular-file symlink and existing quarantine behavior. Verified by the focused `local_drafts` test suite.
- [x] 1.3 Publish quarantine entries with no-replace semantics. Verified that a name collision preserves both source and existing quarantine entry.
- [x] 1.4 Update the client-surfaces developer explanation and narrow the backlog to the remaining check-to-rename window plus slow filesystem calls. Verified the links through strict OpenSpec validation.

## 2. Validate

- [x] 2.1 Run focused Pager local-draft tests and formatting; record results in `verification.md`.
- [x] 2.2 Run `openspec validate --all --strict --no-interactive` and resolve any failures.
