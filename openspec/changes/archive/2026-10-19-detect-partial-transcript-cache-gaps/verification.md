# Verification

## Scope and implementation

The original backlog gap was undetectable partial `updates.jsonl` loss. The offline reader now compares exact committed input/turn identities, input echo block count and digest, and typed Workflow/Goal/Control/CurrentMode projection expectations at captured Timeline/cache frontiers. It reports an incomplete-cache error and never starts an actor, guesses a neighboring row or rewrites source history. Historical facts without supported identities do not gain invented completeness guarantees.

Producers reserve publication capacity before recording an expectation, then require its durable ACK before live/cache publication. Ephemeral and pre-persistence coalesced updates have no durable expectation. The expectation stores identity and digest, never the display body or execution permission. The architecture/development descriptions link this contract.

## Executed evidence — 2026-10-19

Pinned Rust/Rustdoc 1.93.1, two build jobs, no incremental/dev/test debug, 16 MiB test stack. Last complete Shell82 run passes **4219 tests, 0 failed, 5 ignored**; ChatState82 passes **564/0/2**; Storage42 passes **362/0/1**. The newer producer/history tests below are independently run, not inferred from those totals.

- Exact input/terminal and fingerprint cases pass by name in Storage42 and Shell82: `exact_prompt_identity_detects_missing_echo_and_ignores_unrelated_echo`, `echo_fingerprint_detects_truncation_and_missing_blocks`, `ended_turn_requires_its_exact_turn_completed_identity`, `synthetic_workflow_turn_maps_terminal_by_durable_prompt_id`, both rewind exclusions, and `recovery_terminal_does_not_require_turn_completed_row`. ChatState82 also passes `checkpoint_rejects_prompt_id_conflicting_with_started_turn`.
- Real wire/read-only/frontier cases pass: `partial_display_cache_requires_exact_committed_prompt_identity_and_is_read_only`, `partial_display_cache_uses_rewind_selected_branch`, `captured_display_frontier_does_not_adopt_a_later_cache_append`, `expected_projection_requires_exact_captured_row_and_payload`, `rewind_filter_does_not_hide_a_row_from_projection_integrity_check`, and `cold_load_reseeds_from_expectation_missing_from_cache`.
- `goal_update_is_not_published_when_projection_expectation_ack_fails` passes in focus69 and clean Shell82. The actual Goal producer attempts an expectation whose persistence ACK fails; no live/cache row appears and all reserved credits return.
- `ephemeral_and_coalesced_updates_do_not_create_projection_expectations` passes **1/1** in focus91. The actual Workflow sender emits ephemeral and `persist_update=false` rows, then rejects a durable publication when all credits are held. All leave no expectation/cache row; only the first two produce live updates. This covers the producer gate that prevents a phantom expectation for work never published.
- `legacy_workflow_wire_row_without_timeline_identity_is_not_backfilled` passes **1/1** in focus87. A production-wire Workflow row with no Timeline expectation remains readable without fabricated identity, and both source ledgers remain byte-for-byte unchanged.
- CLI73 plus harness76 pass root/tree cold resume and **actual `grow export`** in PTY77. Export accepts the settled canonical root and child sources, includes the original prompt/final result, and does not mutate the ledgers.

Commands use the Cargo no-run generated Shell executable from `crates/codegen/shell`, exact named filter and `--nocapture`; package suites use `--test-threads=4`. Logs are `/tmp/grow-v2-4-0-{shell-final82,chat-state-final82,storage42,focus69}.log`, the named `focus87-*` and `focus91-*` logs, and `pty77-{root-export,tree-export}.log`. New integration tests outside this change retain their own diagnostics; no aggregate success is inferred from an unfinished newer suite.

## Historical diagnostics and final checks

Earlier Shell22 fixtures had incorrect prompt identities or omitted their matching TurnCompleted rows; production wire helpers corrected those fixtures before the passing runs. Shell50/62 failures belong to other test slices and are superseded by Shell82 for this change's existing regressions. Final format92 and strict/archive results are recorded by the root before archive.

Final pre-archive checks: format92 succeeds for the changed Shell/harness sources; full strict92 passes **22/22**, `/tmp/grow-v2-4-0-openspec-all92.log`. Every delta scenario has the named component or process evidence above; unrelated main-feature matrix gaps remain in their own active changes.
