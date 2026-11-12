# Verification

## Baseline regression

Added `public_wait_returns_completed_child_without_cancelling_active_sibling` through the public Tool and real ChannelBackend/coordinator. On the old join_all implementation it failed after 2.02 seconds with `finished child must wake the wait without the active sibling: Elapsed(())`, while its own call budget was 10 seconds. The new implementation passes and confirms the sibling remains running and later completion is still surfaced. A preliminary cargo command used a short filter with --exact and selected zero tests; it is not counted as evidence. The same compiled test binary was then run with the unique test-name filter and selected one failing test.

## Current implementation

Rust commands pin RUSTC/RUSTDOC to toolchain 1.93.1 and use `--offline --locked`, with CARGO_BUILD_JOBS=2.

- `cargo test -p tools --lib public_wait -- --nocapture`: 8 passed. Covers immediate/first terminal, stalled sibling, completed/failed/cancelled/output-unavailable, retained suspended/finalizing snapshots, single-child segmented progression, normalized duplicate IDs, observation failures, shared deadline, consumed IDs, and real coordinator finalization and late sibling completion.
- Same Tools executable: `wait_subagent_waits_past_coordinator_query_segments`: 1 passed.
- `cargo test -p agent --lib observation_guidance_reaches_the_production_role_without_changing_the_head -- --nocapture`: 1 passed across primary/subagent, Full/Extend and renamed capabilities. It checks prompt delivery, not actual model compliance.
- `cargo test -p shell --lib public_subagent_get_reads_only_the_authorized_canonical_cold_result -- --nocapture`: 1 passed through the authorized canonical cold result path, including the new wait reason.
- Same Shell executable: `goal_waiter_cannot_outrun_receipt_admission_or_emit_it_twice`: 1 passed. Completion receipt admission precedes waiter result delivery.

A read-only second review raised a potential simultaneous-completion loss, then withdrew it after tracing the actual Shell receipt admission before coordinator replies. Unreturned terminal siblings retain their durable notification; only returned terminal IDs are acknowledged by the tool-result path. This ordering was reviewed in production code and its existing barrier regression; no new simultaneous model/provider E2E is claimed.

## Boundaries and checks

The four independent observations in audit.md remain backlog items. The timeout/fallback and terminal receipt-ID behavior were exercised by temporary probes linked to the existing production Tools library; probes and generated executables were removed. Full-history copy cost and restored duration are source-level findings; no 1000M-token load or foreground-to-background duration PTY was run.

Shell emitted an existing irrefutable-if-let warning in session/actor/input_admission.rs; that unrelated code was not changed.

Final checks: `cargo check --offline --locked -p shell --tests` passed, including all updated integration-test consumers of the public wait reason. Existing unused-assignment/dead-field warnings in unrelated test fixtures were also reported. `rustfmt --check` on the edited Rust implementation/tests and `git diff --check` passed. Strict OpenSpec validation before archive passed 20 items.

After archive: strict all-spec/change validation passed 19 items; archived-task validation passed 944 items. The backlog links to the archived audit.
