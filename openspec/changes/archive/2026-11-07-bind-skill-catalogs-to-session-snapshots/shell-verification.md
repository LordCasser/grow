# AC-03b Shell verification

## Current status

Shell 实现已通过父级真实 ACP 新绿和局部回归。隔离集成 `session_skills_use_exact_owner_snapshot` 为 1 passed，31.51s；同批 1.93.1 编译后的 unit binary 上 owner/error 三项、既有 Skills 24 项均通过。Pager 与整个 change 的产品链验收仍独立开放。

The four production files were checked against the exact starting SHA-256 values immediately before application. Only the scoped candidate hunks were applied. Existing dirty changes, manifests and Cargo.lock are preserved. No implementation task is checked off before runtime verification.

## Applied artifacts

- `crates/codegen/shell/src/extensions/skills.rs`: six requests accept sessionId, resolve an exact primary and freeze its acknowledged registry before mutation; disk requests build for cwd inside the existing bounded worker with no session roots/latest publication. Existing cwd/null/default, fresh management list and global disabled semantics remain.
- `crates/codegen/shell/src/session/handle.rs`: plugins_list returns `Result<Option<Arc<PluginRegistry>>>`, distinguishing successful None, command closure and reply closure.
- `crates/codegen/shell/src/extensions/plugins.rs`: the sole existing caller propagates communication errors; unknown-session shared fallback remains.
- `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs`: only the Skills dispatch hunk removes shared-latest injection.
- `crates/codegen/shell/src/agent/mvp_agent/tests/skills_catalog_tests.rs`: three error/owner tests, registered with one new `mod skills_catalog_tests;` line in the pre-dirty `tests.rs`.
- `crates/codegen/shell/tests/test_session_skills_plugin_snapshot.rs`: real MvpAgent → duplex ACP fixture, expanded after old-red to cover all six successful session contexts and unchanged cwd parameter behavior. Current SHA-256 `61569d84fe77439b3b6792232b977013e7d2658f73ca6d32df13fd25e5dedf8d`.
- Disposable exact old snapshots remain in `/tmp/ac03b-shell/old/`; prepared candidates remain in `/tmp/ac03b-shell/candidate/`. They are not authoritative over later shared edits.

## Effective old-red completed by parent

```bash
env PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin \
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 \
/Users/lordcasser/.cargo/bin/cargo +1.93.1 test \
-p shell --test test_session_skills_plugin_snapshot --locked --offline \
-- --exact session_skills_use_exact_owner_snapshot --nocapture
```

The parent actually ran the command above on fixture SHA-256 `aba2bb30ef170c2b70757938901f85a7ebed698e3a1cb0bf857f9bdbbe0de39e`, before any production edits. Log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b42b-a03e-7981-828c-93d329398e9b/terminal/call_26d6e0b2b6ff484fb665b3e38b86003d.log`.

Exactly one test ran and failed after 25.26s at line 266, the intended `cwd-only discovery borrowed the shared latest registry` assertion. Actual keys were `latest:common`, `native-only`, `process:common`; expected keys were `native-only`, `process:common`. The fixture's earlier real shared-latest positive control passed. Compilation and startup succeeded, so this is effective behavior old-red, not a parser/startup failure.

The applied expanded fixture retains that trigger. Additional assertions cover A/B different cwd, C/D same cwd with different session pluginDirs, global disabled, all six methods, invalid/missing/retired contexts before mutation, same-cwd disk discovery without session roots, no latest publication, unchanged defaults/nullability, and a real primary acknowledging None.

## 父级实际新绿

编译由固定 Rust 1.93.1、locked/offline、JOBS2、INCREMENTAL0、STACK16MiB 的 Shell 批次完成。父直接执行同批产物 `target/debug/deps/test_session_skills_plugin_snapshot-e73590af53858021 --nocapture --test-threads=1`：1 passed / 0 failed，31.51s。原始日志 `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b42b-a03e-7981-828c-93d329398e9b/terminal/call_cd88e3aa91a2467d90777700ec316251.log`。同脚本后续 SDK 夹具因旧方法名失败，不计入 Skills 结果。

同 unit binary `shell-34fc88a27b856cb5` 的 `skills_catalog_tests::` 3 passed（`call_ffed353743a349ebb4171d5a.log`）、`extensions::skills::tests` 24 passed（`call_beb17760431e4666aeef036a.log`）。两日志位于 runner `01a1b472-6a7c-79c0-8263-bc7b4da5b5f5/terminal/`。其中 worker-failure 用例内捕获的 `scan failed` panic 为预期分支，测试实际通过。父已核源码归属、纯构建与错误边界并接受 Shell 功能；全 change 仍等 Pager 与最终产品链。

## 初始排队命令（历史）

The parent will batch Shell integrations/SDK, then compile the Shell library once and run the three new cases, existing Skills tests and exit-consumer read-only probes. Relevant filters are:

```bash
# Same fixed environment as above.
cargo +1.93.1 test -p shell --lib --locked --offline skills_catalog_tests -- --nocapture
cargo +1.93.1 test -p shell --lib --locked --offline extensions::skills::tests -- --nocapture
```

以上为最初排队命令，实际新绿见上节。既有 saved-after-reload-error 映射保持；本次没有新增持久化失败后的错误映射验证声明。

## Checks actually performed

- Loaded software-engineering entry, Rust principles and handoff contract; read root AGENTS, OpenSpec index, this change proposal/design/specs/tasks and relevant archived Skills contract.
- Checked the bounded call chain: ACP latest injection; six Skills handlers; exact primary resolver; PluginsList command/owner reply; wrapper and sole plugins caller; pure build_for_cwd and the existing real ACP LSP fixture's session metadata/harness.
- Fixed Rust 1.93.1 rustfmt formatting and candidate equality checks passed after application for the production files, expanded integration fixture and three small tests. This is syntax/format evidence only, not compilation. The pre-dirty tests.rs received only the module registration hunk and was not globally formatted.
- `git diff --check` passed over the four production files and module registration. Before edits, all four production SHA-256 values matched the recorded starting snapshots:

| File | SHA-256 |
| --- | --- |
| extensions/skills.rs | `4350119679c0e07ed2b99654142c85b5942eba4d73b8643e885d2920ddc1e3fb` |
| extensions/plugins.rs | `42b87b7ebe746d1263f507afbb7858ebe7cbd57437d89f5493b36529a48d1eae` |
| session/handle.rs | `f63cec479115cb8b0767d5ff911efa523996e8cba0a502e6ca28e8ed5d704992` |
| mvp_agent/acp_agent.rs | `45c6bdd6fbf557cd883610970fd9b52384224111f476c74c0280f7b1c1a66cc7` |

## Remaining gates

Shell 编译、新绿、owner/error 回归与实际 diff 父审已完成。Pager、真实 UI→Shell→模型产品链和整个 change 的 strict/archive 仍待完成。本执行者没有修改主规范、docs、backlog、Pager、AC-06、hooks_plugins、SDK、manifest 或 lock，也未归档或提交。
