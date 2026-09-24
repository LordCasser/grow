## 1. Correct active fuzz documentation

- [x] 1.1 Update the fuzz README's coverage summary and target comment from `render_all.rs`; verified the stated calls, chunk sizes, UTF-8 boundary behavior, and missing oracle against the source.
- [x] 1.2 Update the fuzz manifest's target comment to match the same four no-Syntect paths; verified the documented cargo-fuzz run and crash-reproduction forms against cargo-fuzz's `Run.corpus` argument handling and the manifest/target setup.

## 2. Validate and archive

- [x] 2.1 Run `git diff --check` and focused OpenSpec validation; record the repository-wide strict validation result and that no Cargo build or fuzz campaign was run.
- [x] 2.2 Archive this documentation-only change with `--skip-specs`; the archived artifact is present and archive validation is recorded in `verification.md`.
