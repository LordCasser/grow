# Verification

## Baseline and implementation

Source baseline: main `e7b42299`, clean before this change. Rust/Cargo 1.93.1 on macOS aarch64, rustup proxies explicitly first in PATH. Task-owned target: `/tmp/grow-241-warning-cleanup-target`.

`cargo check --locked -p cli --bin grow --message-format=json` succeeded with one `dead_code` diagnostic naming `AgentSession::prompt_status_query_matches` and `prompt_status_query_matches_generation`. The compiler output and source call sites confirm both wrappers are used only by Pager unit tests. Production uses the `*_for` methods with optional prompt identity for lifecycle queries. Logs: `/tmp/grow-241-warning-baseline-cli.jsonl` and `/tmp/grow-241-warning-baseline-cli.log`.

The implementation adds only `#[cfg(test)]` to those two wrappers, matching adjacent test helpers. It leaves the production methods, query generations, watchdog routing and existing test assertions unchanged; it adds no lint suppression, dependency or gate change. The same default CLI check then passed with **zero compiler warnings or errors**. Logs: `/tmp/grow-241-warning-fixed-cli.jsonl` and `/tmp/grow-241-warning-fixed-cli.log`.

## Workspace targets

`cargo check --locked --workspace --all-targets --message-format=json` passed with **zero compiler warnings or errors**, including unit/integration targets, examples and benches on this host. The JSON build-finished result is true; stderr contains no build-script warning. Logs: `/tmp/grow-241-warning-workspace.jsonl` and `/tmp/grow-241-warning-workspace.log`.

## Isolated regressions

With `RUST_MIN_STACK=16777216` and four test threads, existing Pager unit-test filters `prompt_status` and `lifecycle` passed **16** and **102** tests respectively, with zero failures or ignored tests. Both JSON build results are successful and contain zero compiler warnings/errors; stderr contains no build-script warning. The runs assert a nonzero passing test count rather than treating an empty selection as success. Logs: `/tmp/grow-241-warning-isolated-prompt-status-tests.{jsonl,log}` and `/tmp/grow-241-warning-isolated-lifecycle-tests.{jsonl,log}`. No new mirror test was added for the two cfg attributes.

## Native distribution configuration

`CARGO_INCREMENTAL=0 cargo build --locked --profile release-dist --features release-dist -p cli --bin grow --target aarch64-apple-darwin --message-format=json` passed after a complete 12m 57s build. The JSON build result is true, with **zero compiler warnings or errors**; stderr also has no build-script/linker warning. Logs: `/tmp/grow-241-warning-release-dist.jsonl` and `/tmp/grow-241-warning-release-dist.log`.

The resulting native executable passed `--version`: `grow 2.4.0 (e7b42299) [stable]` (111,085,240 bytes). This checks the candidate working-tree source and native linking, not a final versioned release asset. The build does not run CI's bundled-tool preparation, signing, archive/attestation/checksum validation or publication. Product version remains 2.4.0; nothing was installed. Smoke log: `/tmp/grow-241-warning-native-smoke.log`.

## Final isolated checks and cleanup

The isolated candidate repeated both `cargo check --locked -p cli --bin grow --message-format=json` and `cargo check --locked --workspace --all-targets --message-format=json`. Both passed with **zero compiler warnings/errors and zero stderr build-script/linker warnings**, excluding main's concurrent audit probes. Logs: `/tmp/grow-241-warning-isolated-cli.{jsonl,log}` and `/tmp/grow-241-warning-isolated-workspace.{jsonl,log}`. The aggregate report is `/tmp/grow-241-warning-final-report.json`.

Pinned formatting, diff whitespace and strict active/main OpenSpec validation passed (17 items before this change's archive). The final runtime diff is exactly the two cfg attributes. The task-owned Cargo target was removed after every compiler/test process completed, reclaiming 22,665,981,952 allocated bytes (about 21.1 GiB); diagnostic logs remain. No unrelated Cargo cache or user process was removed.

## Concurrent audit isolation

The first unit-test compilation on main did not execute tests: another ongoing audit added `audit-post-v2-4-0-boundaries/regression-probes.rs` through its temporary module, and that fixture passed an `AgentView` where `Box<AgentView>` is required (`E0308`). Logs: `/tmp/grow-241-warning-prompt-status-tests.jsonl` and `/tmp/grow-241-warning-prompt-status-tests.log`. This attempt is not counted as a passing regression. No unrelated fixture was edited to make it pass.

Only this task's two-line patch and five change files were moved to the existing clean `codex/harden-runtime-release` worktree at the same `e7b42299` baseline. Main's independent audit sources were preserved and checked before/after that move. The final isolated checks and tests exclude those temporary probes; the initial main checks are retained only as the first-pass evidence above.

## Platform boundary

The standard release build is `cargo build --locked --profile release-dist --features release-dist -p cli --bin grow --target <triple>` with default features retained. OHOS alone disables default features and uses its dedicated SDK/toolchain. A local macOS build does not execute Windows/Linux/OHOS cfgs or validate their SDK/linker warnings; those still require the corresponding hosted release builds. The independent MCP branch and deferred multi-round elicitation change are outside this work.

## Archive finalization

Archived with OpenSpec 1.11.0 using `--skip-specs` because the change only corrects test compilation scope. Post-archive strict active/main validation passed **16 items** and strict archive validation passed **626 items**, both with zero failures. The commit scope contains only the two-line Pager change and this maintenance archive. Deferred MCP work and main's independent audit/backlog changes are excluded. No push, tag, installation or publication was performed.
