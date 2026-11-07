# Pager Settings verification — Task 4

## Status

Settings模块已由父审查并接受；Settings真实PTY现也通过，整个特性仍待迟答里程碑验收。初始Settings任务只写源码没有启动Cargo/check/fmt；随后统一验证窗口 `b129` 在组合后的最终源码执行了以下检查。

- `cargo +1.93.1 test -p pager --lib non_blocking_preference --locked --offline`：修正夹具的TOML文档解析调用后，**5 passed / 0 failed**（日志585行起末轮，exit0）。
- `… --lib dispatch::tests::settings`：新增设置后覆盖计数42→43，原行为断言保留；末轮 **125 passed / 0 failed**，exit0。
- `… --lib settings_modal`：**173 passed / 0 failed / 1 ignored**；`… --lib registry`：**67 passed / 0 failed**。各filter包含既有场景及重叠，不相加。
- `cargo +1.93.1 check -p pager -p pager-minimal --tests --locked --offline`：exit0。

父亲读最终设置注册、快照传播、writer effect、保存成功/失败反馈和隔离子进程测试，核真实helper→pubsetter→raw保留→初始loader重读及malformed文件失败回滚。结果来源 `/tmp/grow-ask-pager-card-regress.log` 和 `/tmp/grow-ask-pager-card-tests-check.log`。初轮两个夹具失败保留在日志，没有将其称为生产修复或删掉场景。

fresh CLI的实际F2→保存false→下一实际ask强制blocking→真实选答后继续，以及干净退出/coldcontinue后仍强制blocking，已在 `verification-pty.md` 第三轮单独通过（该轮Settings 1pass、迟答用例1fail）。保存后保留timeout/无关raw字段、两次canonical工具各单次结算均有实际证据；迟答主链未通过不能抹去这个独立通过结果。环境覆盖/typedinvalid由Shell runtime集成另证，不把PTY无override当成env优先级测试。

## Implemented source

- Settings → Agent registers `toolset.ask_user_question.allow_non_blocking`, label `Allow non-blocking questions`, shared default `true`, searchable keywords, reset action, and `restart_required=false`.
- Action → dispatcher optimistically mirrors the selected preference in App/PagerLocalSnapshot, refreshes open Settings, and emits the existing `Effect::PersistSetting` with the previous value for rollback.
- The effect helper awaits `shell::util::config::set_ask_user_question_allow_non_blocking(bool) -> Result<()>`. Shell owns raw TOML preservation and effective per-ask policy; Settings does not rebuild an Agent or decide an admitted question's mode.
- Before completion, the toast says `Saving non-blocking question preference…`. Only the actual `SettingPersisted` result after the last queued write confirms `Saved non-blocking question preference (environment may override)`. Existing failure dispatch restores the previous preference and shows the write error. Description explicitly names `GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING` precedence.
- Startup reads the TOML preference into the App mirror. Settings open/refresh/reset/snapshot paths and dashboard snapshot propagation carry the field.

## Module test coverage

The Settings owner prepared the following tests; the later shared build window executed them as recorded above.

- `app::root::dispatch::tests::settings::non_blocking_preference_action_persists_and_failure_restores_modal`: actual Settings action produces the exact key/value/rollback, pending toast has no success claim, failed result restores the modal and reports the error.
- `app::root::dispatch::tests::settings::non_blocking_preference_failure_keeps_previous_disabled_choice`: failed enable restores a saved disabled preference.
- `app::root::dispatch::tests::settings::non_blocking_preference_success_confirms_only_saved_preference`: actual success confirms saved preference and explicitly retains environment override qualification.
- `app::root::event_loop::tests::non_blocking_preference_persists_and_initial_snapshot_reloads`: existing exact-test isolated-child fixture supplies temporary `GROW_HOME`; actual action → effect helper → public Shell setter writes config, retains timeout fields and unknown nested/raw fields, and the startup loader plus snapshot reads `false`. Real malformed-TOML failure preserves the document and restores the previous disabled UI preference.
- `views::settings_modal::tests::non_blocking_preference_is_searchable_and_emits_toggle_action`: keyboard filter finds the row and Space emits the typed action.
- Existing Settings exhaustive bool/reset/default/row tests have their new key/branch adapted; shared default and live metadata are pinned by the registry default test.

The corresponding targeted filters were later run in the parent-assigned window; actual commands, results and fixture corrections are recorded in Status above. The original handoff listed these selectors:

```sh
cargo test -p pager --lib non_blocking_preference
cargo test -p pager --lib app::root::dispatch::tests::settings
cargo test -p pager --lib views::settings_modal::tests
cargo test -p pager --lib settings::registry::tests
```

## Checks actually performed

- Read the shared software-engineering entry, Rust principles and handoff contract, OpenSpec index, applicable archived configuration/client-surface specs, this change's design/client-surfaces delta/Task 4 handoff, and the existing Settings timeout implementation/callers.
- No nested `AGENTS.md` found under `crates/` or `openspec/`.
- Scoped `git diff --check` across Settings-owned modified files passed (exit 0).
- Static symbol/caller inspection confirms the Shell public setter and Tools default constant exist and the new setting follows the established Settings persistence queue and failure-result path. No runtime behavior is inferred from this check.

## Ownership and remaining gates

- `app/actions.rs` and `app/root/dispatch/router.rs`: only the new Settings action/import/branch were added, then write ownership was explicitly released to parent/b03a immediately.
- Parent authorized only mechanical field propagation in `dispatch/dashboard.rs` and the narrow successful-result toast in `dispatch/task_result.rs`; both are implemented.
- The original handoff left `dispatch/prompt.rs` to the Pager owner, including local App capture and `PagerLocalSnapshot.ask_user_question_allow_non_blocking` propagation. The combined source later included that adaptation and passed pager/pager-minimal tests checks; it is no longer an open Settings compilation dependency.
- No Tools/Shell source, ACP/card/QuestionView, model-pool implementation, real session/user configuration, commit, spec/tasks/archive lifecycle, or global formatting was changed by this task.
- Parent retains Tier 3 verification: Settings save → next real ask observes Shell policy (including forced blocking and env precedence), already admitted questions retain their mode, restart remains persistent, and feature-wide review/acceptance.
