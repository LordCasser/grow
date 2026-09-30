# Verification

- `RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib -- --test-threads=4`: 7,421 passed, 0 failed, 12 ignored. This includes Goal cache-rate helper, status line, detail overlay, and transcript projection regressions.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: 16 items passed before archive.

The behavior-mode status routing was inspected: a present Goal uses `goal_status_line`; otherwise a root session, including Plan or Workflow without Goal, uses `session_usage_status_line`. No separate Plan/Workflow usage renderer was found.
