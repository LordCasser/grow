# AC-07 verification

## Current status

父已取得有效旧红与同一真实 ACP 夹具新绿（1 passed，6.55s），并核对实际 Update 唯一增量后接受行为实现。actor Update 复用 `plugin::update_plugins_by_selector`、现有 typed outcomes 和 `repo_update_requires_reload`，不再独立加载/保存 registry 或直接更新 Git。Install/Uninstall 与 `plugin.rs` 的变更属于独立 AC-08，不计入本项。开发说明与 backlog 已同步，本项已归档。归档后全量 strict 31/31、archive strict 912/912 通过；定向既有 `plugin::tests::update_requires_reload_for_changed_repo_updates_only` 1/1 通过。

## Fixture and acceptance

`crates/codegen/shell/tests/test_acp_plugin_update_results.rs` contains one test, `acp_update_adopts_branch_result_and_preserves_noops`. It uses `MvpAgent::new`, ACP duplex initialization/authentication/session creation and `grow/plugins/action`, reaching the actual session handle and actor. It first installs two old branch snapshots from isolated local bare repositories, advances both remotes, and selects one plugin by the last ID segment.

Before the adoption assertion it checks that checkout HEAD reached the new remote commit and prints the checkout, recorded commit, timestamp and plugin map. An effective old-red result must fail because durable registry commit differs from this advanced checkout, not because compilation, transport or fixture setup failed. After the production fix, assertions cover saved commit/timestamp/name/subdir/version map, selected-repo scope, a second tip no-op with an unchanged saved entry, None/all selection, empty/not-found, an actual SHA installation, a local snapshot, and no implicit session registry reload.

All phases share one temporary HOME/GROW_HOME because `grow_home()` is cached. Git global/system config is isolated, all Git subprocesses are synchronously waited, and no external LSP/MCP command or prompt is configured. Successful completion closes the session; the owning LocalSet/runtime drop cancels remaining ACP tasks, including on stack unwinding. Environment/cwd guards restore previous values before the temporary home is removed.

## Queued command

Run from the repository root, after the parent's Cargo window release:

```sh
env PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo +1.93.1 test -p shell --test test_acp_plugin_update_results --locked --offline -- --exact acp_update_adopts_branch_result_and_preserves_noops --nocapture
```

The test runner must report exactly one executed test. The parent ran the command on the preserved old actor; the same test must now run after the actor consumes the shared application outcomes.

## Effective old-red evidence

Raw parent log:
`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b42b-a03e-7981-828c-93d329398e9b/terminal/call_a4230ab554ef433984b9843e0df7735d.log`

The log reports `running 1 test`, then:

```text
AC-07 adoption evidence: checkout=efc64f6aaf127b1169489a5577e34685566d817a, recorded=d7343374d458fe500b04fb3ca7c003508ed0de94, updated_at=2000-01-01T00:00:00+00:00, plugins={"selected": RepoPlugin { subdir: Some("original-dir"), version: Some("1.0.0") }}
```

It fails at fixture line 358 with `ACP updated checkout but did not adopt its commit into durable registry`: recorded `d7343374d458fe500b04fb3ca7c003508ed0de94` differs from checkout `efc64f6aaf127b1169489a5577e34685566d817a`. Final test result is 0 passed, 1 failed, 0 ignored, 0 filtered out, 6.46s. This proves the intended regression through the actual ACP/handle/actor path. An earlier parent command was terminated at its 120-second compile timeout; it is not counted as old-red evidence.

## Checks actually performed

- `/Users/lordcasser/.cargo/bin/rustfmt +1.93.1 --edition 2024 crates/codegen/shell/tests/test_acp_plugin_update_results.rs`: exit 0.
- `git diff --check -- crates/codegen/shell/tests/test_acp_plugin_update_results.rs`: exit 0 (tracked-diff check; the fixture is a new file).
- `rustfmt +1.93.1 --edition 2024 --check crates/codegen/shell/tests/test_acp_plugin_update_results.rs`: exit 0.
- `git diff --no-index --check /dev/null <new-file>` for the fixture and this verification record: each raw exit 1 with empty diagnostics (normal new-file differences); the wrapper checking diagnostics exited 0.
- Scoped source inspection confirms `PluginsAction` uses nested `plugin_id` and ActionOutcome uses camelCase fields; the fixture uses those actual wire names.
- After the production branch replacement, `/Users/lordcasser/.cargo/bin/rustfmt +1.93.1 --edition 2024 --check crates/codegen/shell/src/session/actor/hooks_plugins.rs crates/codegen/shell/tests/test_acp_plugin_update_results.rs`: exit 0. An initial check requested only wrapping the local snapshot message; that formatting was applied before the successful check.
- `git diff --check -- crates/codegen/shell/src/session/actor/hooks_plugins.rs`: exit 0. Final source diff inspection confirms the AC-07 production edit is confined to the Update branch; existing async config and hook snapshot edits are preserved.
- Existing unrelated dirty changes in `hooks_plugins.rs` (async config calls and plugin hook replacement) remain preserved.

## Remaining boundaries

父级已完成同一真实 ACP 夹具的新绿：`acp_update_adopts_branch_result_and_preserves_noops` 1 passed，6.55s，checkout 和 durable registry 均为 `3281346fb45635c36739b3d134f3b6e391b99a24`，新插件及 version map 已采用，后续 no-op/selector/pinned/local 与无隐式 reload 断言全部通过。原始日志 `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b472-6a7c-79c0-8263-bc7b4da5b5f5/terminal/call_c9b706a6a1a7490ea8dc2fef.log`。父已核 Update 唯一生产增量、共享 owner 采用及 outcome/reload 消费，接受本项更新采用行为；未新增 marketplace 事务逻辑，单独的 marketplace/provenance 回归尚未在本批重跑。AC-08 保存错误不在本项验收范围。
