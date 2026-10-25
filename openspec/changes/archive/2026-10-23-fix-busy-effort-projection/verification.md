# Verification

The reported local session recorded a `high → max` Sampling change, followed by `max → max` selections with no notice. This matches the reported stale `high` footer and the existing no-op notice rule. The Pager regression reproduced `High` remaining visible after the exact applied control receipt before the fix, then passed after the fix.

- `cargo test --locked -p pager --lib effort_patch_terminal_applies_parked_model_change_with_other_control_pending`: passed after the final source and test edits. Covers model notification before receipt, a foreign receipt, the matching receipt, and an unrelated pending Agent control.
- `cargo test --locked -p pager --lib app::acp_handler::tests::models::`: 19 passed.
- `cargo test --locked -p pager --lib app::acp_handler::tests::session_events::`: 50 passed, including child control and replay cases.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed before archive.

Review: Only an accepted terminal carrying the exact local intent can release the parked projection. A foreign receipt cannot clear it. Root and child notifications share the corrected function; other pending control domains do not delay Sampling projection. Shell still owns the step boundary and the success/no-op notice.
