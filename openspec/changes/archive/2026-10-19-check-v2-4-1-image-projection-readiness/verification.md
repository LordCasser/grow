# Verification

## Source and default-build failure

Runtime baseline is main `565bbd240454467edbd39a024057565bbcfaa11f`, with prior authorized audit in `cd98797f`. All five original implementation/test files and their specification/doc/archived-change hunks were reviewed and committed separately; main was clean afterward. Checks run in the existing managed `codex/harden-runtime-release` worktree fast-forwarded to that main SHA. Only this maintenance change, its import correction and development note are additional; no unrelated dirty build input is used.

On macOS arm64 with Rust/Cargo 1.93.1, rustup proxies explicitly first in PATH and task-owned `/tmp/grow-241-projection-preflight-target`:

- `cargo check --locked -p cli --bin grow --message-format=json -j 6` failed: three E0433 diagnostics for production `sampling_types` references in AgentMessageBlock, plus two consequent E0282 diagnostics. Pager declares that crate only under dev-dependencies. Logs: `/tmp/grow-241-projection-preflight-cli.{jsonl,log}`. This is a failing preflight, not a passed regression.
- The same command after importing the identical constant via existing `shell::sampling::conversation` succeeded: build-finished=true, zero compiler warnings/errors, zero stderr build-script/linker warnings/errors. Logs: `/tmp/grow-241-projection-preflight-fixed-cli.{jsonl,log}`.
- `cargo check --locked --workspace --all-targets --message-format=json -j 6` succeeded: build-finished=true, zero compiler warnings/errors and zero stderr warnings/errors. Logs: `/tmp/grow-241-projection-preflight-workspace.{jsonl,log}`. Aggregate: `/tmp/grow-241-projection-preflight-build-report.json`.

The prior 312 + 568 core passes, 45 child-context passes, 16 assistant-block passes, 164 tracker passes and 21 combined main projection passes remain recorded in the image change. They were executed on the original behavior implementation; this follow-up changes only its import path through a direct re-export and compiles both default and test targets. No new mirror unit test was added for a use statement, and those functional suites were not falsely counted as newly rerun. The non-test failure explains why library tests alone missed the build issue; the existing core workflow's separate default CLI build would reject it.

## Release and document checks

- `python3 -m unittest discover -s scripts/tests -v`: 18 passed, covering exact test runner and release DAG, source SHA, gate cancellation/skips and Windows trigger paths. Log: `/tmp/grow-241-projection-preflight-python.log`.
- `bash -n scripts/validate-release.sh` passed. The independent Python static contract extracted from that script passed against current workflows and the existing 2.4.0 notes file. This checks catalog, assets, fail-closed publication and Markdown format; it does not assert 2.4.1 notes exist or bypass the full script's missing-tag/version requirements.
- Six workflow YAML documents parsed using system Ruby YAML/Psych; 18 release/build inline Bash blocks passed `bash -n` after inert substitution of GitHub expressions. Four pwsh blocks were not parsed/executed locally and remain native Windows validation work. Log: `/tmp/grow-241-projection-preflight-workflows.json`. System Python has no PyYAML; no package was installed merely to perform syntax checks.
- Locked metadata/lock identity passed for 57 packages (33 product-version inherited, 24 independent). Product version remains 2.4.0; 2.4.1 notes are absent. Report: `/tmp/grow-241-projection-preflight-version.json`.

Pinned `cargo fmt --all -- --check` and `git diff --check` passed. The v2.4.0-to-candidate changed Markdown set plus project entry points and this new maintenance change passed relative target/anchor validation; external URLs and historical absolute diagnostic references are outside that local check. The exact counts and final archive checks are recorded below. No unrelated architecture debt was implemented.

## Finalization

160 Markdown documents passed checks for 299 relative targets and 175 anchors, with seven external URLs outside the local check. The first active-change link check caught a relative link written for its future archive location; it was corrected before the passing check, then will be relocated and rechecked during archive. Log: `/tmp/grow-241-projection-preflight-links.json`.

Strict active/main OpenSpec validation passed 17 items before archive. The task-owned Cargo target was removed after compiler/formatter processes completed, reclaiming 9,693,350,718 bytes (9.0 GiB). No global Cargo cache or user processes were removed. Python bytecode created by this task was removed.

Post-archive strict active/main validation passed 16 items; archive validation passed 636 items, both with zero failures. Markdown targets/anchors passed again after relocation; diff whitespace checks passed. The final maintenance commit contains only AgentMessageBlock's existing-type import path, the development note, and this skip_specs archive. It is integrated by fast-forward into clean main; no runtime source is changed during integration. The resulting SHA is left to Git history rather than embedding a self-reference. The coordinating audit task owns the subsequent main push under its user's authorization; tag/release remain prohibited for this round. No tag, installation, release workflow or publication was performed.
