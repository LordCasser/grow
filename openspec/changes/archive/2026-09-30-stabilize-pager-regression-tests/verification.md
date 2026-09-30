# Verification

- The Settings list expectation includes the already registered `show_model_provider` row in its Models order. Runtime registry and Settings behavior did not change.
- The per-view inline-media pending-cap test seeds two pending paths directly, then checks that a third request is neither admitted nor marked failed. It no longer depends on process-wide worker permits shared with parallel tests. Runtime admission code did not change.
- Pager library suite, single-threaded: **7417 passed, 0 failed, 12 ignored**.
- Pager library suite, default parallel execution: **7417 passed, 0 failed, 12 ignored**.
- Focused Rust formatting and `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed before archive.
