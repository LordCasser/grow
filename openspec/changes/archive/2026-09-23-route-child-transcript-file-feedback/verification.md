# Verification

- `cargo test -p pager --lib child_export -- --test-threads=2`: 2 passed during development. The Minimal child test uses distinct parent/child content and cwd, switches to the parent before completion, and checks the child alone receives saving/success notices. The rebound test initially failed because the existing child map lookup still matched the old key after `session_id` changed; requiring the view's current session ID fixed it.
- After adding a removed-child assertion, `cargo test -p pager --lib app::root::dispatch::tests::transcript:: -- --test-threads=4`: 26 passed, including both child tests, deferred writes, queue ordering, removed-root failure, cwd routing, and pager feedback cases.
- `cargo test -p pager --lib input_diagnostic_child_records_and_dump_resolves_same_surface -- --test-threads=1`: 1 passed after setting Minimal mode; confirms a key event and its diagnostic snapshot belong to the active child.
- Static source check: Minimal rendering still reads the top-level Agent through `minimal_api::app_agent(_mut)` and `with_minimal_live_state`, while input delegates to `active_subagent`. This distinct draw bug is narrowed in `openspec/backlog.md`, not altered by this file-feedback change. A production child-view removal path was not found, so stale focus after removal is not claimed as a user-reachable defect here.
- `rustfmt --edition 2024 --check` on the three changed production dispatcher files: passed. The existing transcript test file has unrelated baseline formatting differences, so formatting the whole file would expand the diff.

The file queue remains serial and writes the frozen snapshot even if its view is later rebound; only stale user-facing feedback is suppressed.
