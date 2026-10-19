# Verification

Core [39275706329](https://github.com/LordCasser/grow/actions/runs/39275706329) on fb1beab8 stopped in the shell suite: 4284 passed, one failed, seven ignored. `config_watch_late_directory_and_recreation` failed at “unwatch must prevent revival”; tools tests had not executed. The received event path was not logged, so this is not a reconstructed OS event trace. Source confirms that queued config.toml events previously bypassed the ownership check used for .grow directory reattachment.

## Deterministic red/green and real OS tests

Pinned Rust 1.93.1, target `/tmp/grow-241-dump-boundary-target`:

- Extracted the existing path match without changing its semantics. The new test registers a real watcher, handles an active config path, unregisters its project, then feeds the queued paths through the same private method used by the production callback. It fails against the original match: expected None, received ProjectConfigChanged (zero passed, one failed).
- Added live source ownership checks and retained the independent global/explicit directory watches on project release. `cargo test --locked -p shell --lib config::watcher:: -- --nocapture`: **14 passed, zero failed/ignored**, zero compiler warnings/errors. The permanent-source test writes actual files after project unregister, covering both separate global/project directories and a shared directory. Existing late creation/recreation, aliases, shared repository ancestor ownership and discovery replacement tests pass.
- Original failing late-directory/recreation/unwatch test: **10/10 additional direct test-binary runs passed**, each bounded by a 30-second subprocess timeout. This is targeted verification of an observed race, not a replacement for the full hosted gate.
- Default `cargo build --locked -p cli --bin grow`: passed, zero compiler warnings/errors. Changed Rust formatting and Python release/runner tests are included in final preparation checks.

## Adversarial scope

Root and root/.grow permanent ownership are checked independently; one retained watch cannot keep an unrelated unowned watch. Permanent owners permit config.toml delivery but do not authorize .grow project reattachment without an active project root. The existing shared ancestor reference check remains. Already delivered receiver-queue messages are not retracted. Static extra-directory late creation/replacement capabilities are not expanded.

Call-site review found production watch_path registrations, but no production unwatch_path call. This validates the public watcher API and existing contract/tests; it does not claim production session teardown now reclaims watches. That lifecycle debt is recorded separately in backlog. MCP SDK/policy source and branch are untouched. Final corrected-source hosted gates remain required for publication.

Final formatting and whitespace checks passed; Python runner/release contract tests: 19 passed. Strict current/change validation: 16 passed; archived validation: 641 passed.
