## Verification

- `cargo test --locked --offline -p pager --lib switch_agent_ -- --test-threads=1`: 9 passed. This includes immediate built-in choices, stale request rejection, closed picker/session-binding rejection, in-place filter preservation, and failure fallback.
- `cargo test --locked --offline -p pager --lib workflow_agent_picker_uses_snapshot_without_discovery_effect -- --test-threads=1`: 1 passed. Workflow child choices come only from its frozen Run snapshot.
- `cargo test --locked --offline -p pager --lib agent_catalog_timeout_keeps_blocked_worker_permit -- --test-threads=1`: 1 passed. The shared injected worker test verifies a stalled scan retains the permit after the request deadline.
- `cargo fmt --all -- --check`, `git diff --check` on affected files, and `openspec validate --all --strict --no-interactive`: passed.

Code search confirms `build_switch_agent_catalog` now has only the background effect as a runtime caller; PromptWidget construction and picker opening call only the static built-in catalog. The worker timeout test uses a controlled blocked closure rather than a slow filesystem.

Archived as `2026-09-24-load-switch-agent-catalog-off-ui-thread`; the accepted client-surfaces spec now includes switch Agent discovery ownership.
