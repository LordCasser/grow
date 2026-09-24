## Verification

- `cargo test --locked --offline -p pager --lib agent_modal_loads_without_scanning_on_open_and_ignores_stale_results -- --test-threads=1`: 1 passed. Opening emits a load effect before filesystem discovery; a closed/reopened modal rejects the earlier token.
- `cargo test --locked --offline -p pager --lib config_editor_refresh_queues_agent_catalog_load -- --test-threads=1`: 1 passed. Returning from the editor begins a new tokened load and queues its effect.
- `cargo test --locked --offline -p pager --lib agents_modal -- --test-threads=1`: 4 passed.
- `cargo test --locked --offline -p pager --lib agent_catalog_timeout_keeps_blocked_worker_permit -- --test-threads=1`: 1 passed. An injected blocking scan times out without freeing its worker permit; a queued scan also times out until the first worker exits.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed.

The UI reducer test does not emulate a slow filesystem directly; the injected worker test verifies the blocking and cancellation boundary, and the reducer tests verify modal ownership separately.

Archived as `2026-09-23-load-agents-modal-off-ui-thread`; the accepted client-surfaces spec now includes the modal discovery requirement.
