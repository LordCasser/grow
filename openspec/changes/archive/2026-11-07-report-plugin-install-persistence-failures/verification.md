# AC-08 ordinary Install verification

## Current state

Production Install fix applied after two actual old-red failures were confirmed. Shared `install_plugin` now uses the existing fallible registry loader and returns the existing `InstallFailed` error with repo/path/save context after publication; only a successful save reaches post-install. Actor Install directly awaits this application and retains its input validation, warnings formatting and outcome mapping. The CLI consumer already propagates the shared error and required no production change.

The parent owns Cargo scheduling. This executor has not run Cargo. New-green runtime checks remain pending in the parent's next Shell/Pager batch. CLI old-red was not run; the two actual owner failures are sufficient to implement the frozen fix, and the CLI public consumer must still pass on the new implementation.

## Prepared checks

All three checks self-reexec with isolated `HOME`, `GROW_HOME`, cwd and a fresh process before cached configuration is used. Fixture sources and mutations are confined to temporary homes. Registry save failure uses the existing `GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE` hook; no dependency or production fault mechanism is added.

| Cargo package / target | Exact test name | Required evidence |
| --- | --- | --- |
| `shell --test test_plugin_install_persistence` | `shared_install_reports_persistence_failures` | Actual local snapshot published before save fault; prior registry/config bytes and prior artifact preserved; shared application returns an error containing repo/path and incomplete-publication facts. Corrupt JSON and a `registry.json` directory must stop before publication. Normal names/path/config enablement, local snapshot isolation, process require-SHA refusal and post-install warnings remain covered. |
| `shell --test test_plugin_install_persistence` | `acp_install_reports_persistence_failures` | Actual ACP extension → session handle → actor Install. Fault returns InternalError with reload/restart false, actual new artifact remains, old registry/config remain. Read/parse controls stop publication. Success and auto-enable warnings retain Success/reload=true and existing message formatting; empty source stays ValidationError. |
| `pager --lib` | `plugin_cmd::tests::public_install_dispatch_propagates_persistence_failure` | Public `plugin_cmd::run(Install { trust: true })` in separate save/parse/read/success child processes. Failure preserves registry/config and returns Err, emits exactly one failure diagnostic and no success output; success returns Ok, records the actual artifact and emits the success diagnostic/output. |

The parent can compile each target once in its assigned Cargo batch, then run the resulting executable with each exact test name and `--exact --test-threads=1`. Equivalent standalone selectors, when a Cargo window is assigned:

```sh
python3 scripts/run_exact_cargo_test.py --package shell --test test_plugin_install_persistence shared_install_reports_persistence_failures
python3 scripts/run_exact_cargo_test.py --package shell --test test_plugin_install_persistence acp_install_reports_persistence_failures
python3 scripts/run_exact_cargo_test.py --package pager --lib plugin_cmd::tests::public_install_dispatch_propagates_persistence_failure
```

## Actual old-red evidence

The parent ran the original production implementation with the prepared fixtures. The log was read and confirmed at `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b472-6a7c-79c0-8263-bc7b4da5b5f5/terminal/call_c9b706a6a1a7490ea8dc2fef.log`.

- Compilation succeeded in 1m 31s. The Install target selected exactly two tests; both failed in 6.57s at the intended persistence behavior assertions, after artifact/registry/config comparisons had succeeded.
- `acp_install_reports_persistence_failures`: actual ACP returned `status=success`, `requiresReload=true`, `requiresRestart=false`, with `Installed 0 plugin(s)` and a `repo not found in registry` warning. The assertion expected InternalError (line 286 in that source revision).
- `shared_install_reports_persistence_failures`: actual shared application returned Ok, names `[]`, and the same missing-repo warning after the injected registry save failure. The assertion reported the published artifact path and unchanged old registry bytes (line 149 in that source revision).
- The same batch's existing `acp_update_adopts_branch_result_and_preserves_noops` passed: 1 selected, 1 passed, 6.55s. This is retained as adjacent AC-07 evidence, not counted as an Install new-green.
- CLI public-dispatch old-red was not executed. No CLI old-red result is claimed.

These are actual old-red behavior failures, not compile, startup or isolation failures. New-green runtime results remain pending.

## Final parent new-green evidence

Rust 1.93.1、locked/offline、JOBS2、INCREMENTAL0、STACK16MiB 下，`/tmp/grow-262-shell-final-integration.log` 实际编译成功，两个 Install tests 均通过，测试体 6.92s。SDK 另一个 target 的序列断言失败使总批次 exit101，不影响这两个已实际通过的独立场景；不报全批成功。

同一 2.6.2 Pager batch `/tmp/grow-262-pager-final-tests.log` 中 public Install dispatch 1/1 实际通过，覆盖隔离 save/parse/read/success 子进程。父已读共享应用 try_load→publish→insert/save→post_install 顺序及 actor 委托、现有 CLI Err 传播，所有者正确，未混入尚延期的卸载或普通更新保存错误修复。

## Static checks performed

- Loaded software-engineering entry, Rust principles and handoff contract; read repository instructions, OpenSpec index and this change's frozen proposal/design/delta/tasks.
- Checked scoped source and callers, existing fallible loader/save fault, require-SHA handling and post-install warning policy.
- `rustfmt --edition 2024 crates/codegen/shell/tests/test_plugin_install_persistence.rs crates/codegen/pager/src/plugin_cmd.rs` completed successfully.
- `git diff --check -- crates/codegen/pager/src/plugin_cmd.rs crates/codegen/shell/tests/test_plugin_install_persistence.rs` completed successfully.

- After applying only the shared Install load/save hunk and actor Install delegation hunk, `rustfmt --check --edition 2024 crates/codegen/shell/src/plugin.rs crates/codegen/shell/src/session/actor/hooks_plugins.rs crates/codegen/pager/src/plugin_cmd.rs crates/codegen/shell/tests/test_plugin_install_persistence.rs` passed.
- `git diff --check -- crates/codegen/shell/src/plugin.rs crates/codegen/shell/src/session/actor/hooks_plugins.rs crates/codegen/pager/src/plugin_cmd.rs` passed. Diff self-review confirmed this executor changed no adjacent Update/Uninstall/marketplace behavior; pre-existing dirty changes in those files remain intact.

Static formatting is not compilation or behavioral evidence. No tasks are marked complete on that basis. Production implementation is ready for the parent's new-green batch. Runtime new-green, parent review, docs/backlog changes, strict validation and archive remain pending under their existing owners.
