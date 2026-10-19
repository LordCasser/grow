# Verification

Local environment: macOS arm64, Rust 1.93.1 (the repository toolchain), isolated Cargo target, CARGO_INCREMENTAL=0 and bounded build/test concurrency. Preliminary runs using the host's newer compiler are superseded by the pinned-toolchain results below. No hosted workflow or release was dispatched.

## Library and process evidence

The eleven gated library packages completed successfully across the pinned full run and the final affected-package rerun:

| Package | Passed | Explicitly ignored |
| --- | ---: | ---: |
| agent | 482 | 0 |
| chat-state | 567 | 2 |
| memory | 302 | 0 |
| pager | 7517 | 12 |
| pager-minimal | 94 | 0 |
| sampler | 259 | 0 |
| sampling-types | 308 | 0 |
| shell | 4266 | 5 |
| tool-types | 123 | 0 |
| tools | 2422 | 7 |
| workflow | 65 | 0 |
| Total | 16405 | 26 |

The final affected-package rerun passed chat-state, Pager and Shell together. It incorporates the receipt-row cursor/replay corrections and the legacy vision recovery fixture correction. A subsequent compile removes obsolete Notice setters; no runtime behavior changes after that build.

The final CLI entry point builds with `cargo build --locked -p cli --bin grow`. The real `scripts/test_local_coordination.py` process regression passes all ten checks, including cross-workspace permission rejection, busy foreground, FIFO, same-ID deduplication, cancellation, real tool lifecycle, crash/reload and peer identity.

## Exact integration selectors

Each selected test is executed through `scripts/run_exact_cargo_test.py`, which requires a unique listed name and an actual one-test pass. Native CLI and loopback fixtures are used, without external credentials.

- `suspend_nested_task_tree::ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree`: passed.
- `ctrl_d_suspend_and_continue::ctrl_d_cold_resume_keeps_the_original_explicit_skill_snapshot`: passed on the final binary, including after fixture correction.
- `ctrl_d_suspend_and_continue::minimal_cold_resume_keeps_the_original_explicit_skill_snapshot`: passed after durable-input fixture selection.
- `plan_approval_restored_after_resume` and `deferred_plan_reviewed_after_resume`: both passed. Their stale single-Ctrl+C fixture was corrected to the current double-Ctrl+D suspension contract.
- `suspend_nested_task_tree::cold_resume_after_partial_leaf_release_does_not_replay_checkpoint`: passed.
- `suspend_nested_task_tree::concurrent_cold_resume_claims_one_root_without_replaying_tree_work`: passed.
- Existing `ctrl_d_suspend_and_continue::ctrl_d_suspends_running_root_and_continue_resumes_without_replay`: also passed after the shared fixture selection correction.

The existing twenty PTY selectors remain in the hosted core workflow. The seven additional selectors above were run locally; this is not a claim that every ignored integration test in the repository was executed.

The Skill fixture initially selected the first directory under isolated HOME, which could be an empty Minimal startup session. It now identifies the unique durable Turn by its input and retains real --continue, original Turn identity, frozen Skill content, no command replay, export and headless assertions. A preliminary normal-mode restore also timed out against an earlier CLI build; this did not reproduce on the final rebuild. Its cause is not claimed to be proven. The fixture retains cold-start debug logs, and the gate has no automatic retry that could hide a future failure.

## Gate and specification evidence

- `python3 -m unittest discover -s scripts/tests`: 16 passed, including a real temporary Cargo fixture. Negative cases cover missing/ambiguous names, listing failure, zero executed tests, default-ignored tests, failure, nested harness output, missing release dependencies/result checks, a wrong commit and cancellation bypass.
- actionlint 1.7.12 validates all five changed workflows.
- All reusable gates receive and verify the release's full immutable commit SHA. Core, three-platform coordination/export, Windows storage and OpenSpec must succeed before publication; skipped, cancelled and failed gates cannot publish.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: 22 passed.
- `openspec validate --archived --no-interactive`: 610 passed.

## Limits and remaining work

Native hosted Windows/Linux jobs have not run in this task. Windows directory-handle behavior is fixed in code and covered by the new native production-export selector, but local macOS execution is not Windows acceptance evidence. Changes remain open for review and platform validation; no commit, push, tag, archive or release was performed.

The generic tool-error propagation debt found during Skill integration is recorded separately in openspec/backlog.md. It requires its own failure-settlement change and was not mixed into these implementations. Ongoing MCP work in the original checkout was not imported or modified.

## Archive decision — 2026-10-19

The user requested archival of the completed implementation changes. The eight changes are archived into the main specifications, with current developer guidance referencing those specifications. The earlier open-state statement above describes the verification time; native hosted Linux/Windows acceptance remains a release prerequisite, not a claim established by archival. The later sampling recovery verification adds the live-binary and child-grant cases without replacing the earlier evidence. Combined-main integration and v2.4.1 readiness are recorded separately in the prepare-v2-4-1-analysis change.
