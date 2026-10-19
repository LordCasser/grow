# Verification

## Evidence — 2026-10-19

The implementation uses member writer leases and a durable component reclaim intent. Read-only `open_session` and `open_session_by_id_shared_read` inspect only an existing intent directory; they do not create `.reclaim-intents` or `.reclaim-gate.lock`. Writer admission checks the intent snapshot, claims the session writer lease, and checks the snapshot again while retaining the lease. Reclaim holds every member lease before publishing the intent.

In `/tmp/grow-v2-4-0-shell22.log`, the compiled Shell test binary completed the listed JSONL tests before the overall test process was terminated. Passing reclaim coverage included `cleanup_child_seed_requires_exact_parent_terminal_result_chain` (closed chain eligibility, explicit single-member delete refusal, incomplete delivery/receipt, mixed TTL, and held writer matrix), `cleanup_reclaims_three_level_chain_with_root_summary_and_direct_seed`, `cleanup_retains_spawn_when_child_source_is_missing`, `committed_session_reclaim_resumes_after_one_member_was_removed`, `committed_session_reclaim_preserves_replacement_identity`, `committed_session_reclaim_waits_for_all_member_writers`, `malformed_reclaim_intent_blocks_session_admission`, and `read_only_admission_does_not_create_reclaim_metadata`.

The same full Shell run reported `unrelated_writer_admission_survives_a_busy_reclaim_decision_gate` failed. After the fixture/decision-gate correction, that exact test passed in `/tmp/grow-v2-4-0-shell-blockers33.log` (**1 passed, 0 failed**). `/tmp/grow-v2-4-0-shell22.log` was later stopped by SIGTERM while the Shell suite was running; it has no suite-wide result and is not evidence that the Shell suite passed. The focused chain tests above passed in that run before termination, but the full required Shell verification remains open.

## Final focused storage regression — 2026-10-19

Executed the build40 Shell library binary with `RUST_MIN_STACK=16777216` from `crates/codegen/shell`:

```sh
target/debug/deps/shell-33bda6e83c52c4e3 session::storage:: --test-threads=4
```

**362 passed, 0 failed, 1 ignored**, 28.87s; `/tmp/grow-v2-4-0-storage42.log`. This includes every named reclaim test above, including the corrected unrelated-writer regression, plus read-only capture and branch/source protection. The executable path in the command is workspace-relative; the actual invocation used its absolute workspace path. The earlier full Shell failure remains historical; this change's storage behavior has a clean focused run. Whole-release and CLI suspension gates remain tracked in `prepare-v2-4-0`.

`openspec validate --all --strict --no-interactive` passed **26/26** after the storage regression and task audit; `/tmp/grow-v2-4-0-openspec-all44.log`.
