# In-process AC06 verification

## Final parent runtime evidence

固定Rust1.93.1/locked/offline，在实际macOS discovery metadata backend及两项review修正后，`/tmp/grow-262-pager-reload-final.log` 编译成功，九项Skills热更新与两项相邻MCP生命周期共 **11 passed / 0 failed / 0 ignored**，59.66s。原八项完整保留，新增A→B→A初始注册失败恢复项；无增加十二秒断言期限。先前五项FSEvents RPC阻塞失败、cwd单变量未解及动态栈全部保留，未以删除场景取得新绿。

父已亲读config/watchowner最小diff，独立review的Noneactivepath误去重及完成output取消Drop均分别修正并验证。共享watcher模块22/22、fresh默认CLI真实leader1/1亦通过，见相邻两份最终记录；不是仅验证路由helper。

## Source status

Implementation and fixtures are ready for the parent's queued Pager compile/run. Cargo has not been run by this coder: the current build slot belongs to PTY, followed by the parent's Shell batch. No runtime pass is claimed by formatting or diff inspection.

Write surface: `crates/codegen/pager/src/acp/spawn.rs` and the new independent `spawn/tests/skills_hot_reload.rs`. Existing dirty work, Shell leader/app/watcher/Skills APIs, Pager epoch/actions/effects/dispatch, manifests and lockfile are untouched. No shared contract or main specification/documentation was edited.

The existing config consumer now routes every `ConfigUpdate::Skills` through `grow/internal/reload_skills`; only changed paths notify a watch channel subscribed before first registration. The existing discovery owner consumes pending latest paths, including while initial/replacement registration is busy. Construction uses `spawn_blocking` and `finish_blocking_registration`; success adopts the replacement and retires the old owner in the blocking pool. Failure preserves the old owner and logs a warning. The owner guard also offloads native Drop during task abort, gateway exit and cancellation. The task stays present when initial construction returns None, so later path updates can create an owner. Existing workflow-directory event handling is preserved.

## Required execution

New filter: `acp::spawn::tests::skills_hot_reload::` — **8 tests**, all enabled, zero ignored. Each test uses the existing exact-test child process fixture to isolate the cached GROW_HOME, with isolated HOME/USERPROFILE for user skill discovery. The parent can compile Pager once, then run this filter with the test binary or `cargo test --locked -p pager --lib acp::spawn::tests::skills_hot_reload:: -- --test-threads=1` after acquiring the Cargo slot.

Exact names under that filter:

1. `config_only_disabled_and_ignore_refresh_callable_and_slash`: real `spawn_shell` session; only config disabled/ignore writes; actual model-requested `load_skill` results and ACP slash publication must converge; unchanged paths must not register another discovery watcher.
2. `custom_root_creation_modification_and_removal_follow_config`: add external root in config; create and modify its skill after native registration; observe session slash changes and actual callable result; remove root and confirm native owner Drop completes.
3. `initial_registration_preserves_latest_paths`: config update is acknowledged by a resident session while initial native registration is held; latest root must later supply real disk events.
4. `replacement_registration_preserves_latest_paths`: a second config update arrives while the first replacement is held; latest root must supply subsequent real disk events, and the intermediate owner must retire.
5. `failed_replacement_keeps_prior_native_owner`: existing private registration gate injects None; the prior native owner must continue delivering real user-skill events without reporting a replacement active.
6. `initial_failure_can_recover_on_later_paths_update`: initial None must leave a consumer capable of registering later valid custom paths.
7. `cancellation_during_replacement_retires_all_native_owners`: while construction is held, cancellation must retire the old owner without waiting for registration; releasing registration must retire its eventual native owner too.
8. `gateway_exit_during_replacement_retires_all_native_owners`: the same lifecycle assertion after ACP gateway input closes.

Preserve existing entry/lifecycle regressions: `acp::spawn::tests::gateway_completion_preempts_blocked_watcher_registration` (1), `acp::spawn::tests::mcp_hot_reload::lifecycle::config_policy_hot_reload_continues_while_skills_registration_is_blocked` (1), and `acp::spawn::tests::mcp_hot_reload::lifecycle::watcher_ready_reconciles_sessions_created_during_registration` (1). These source tests were not modified.

## Checks actually performed

- `rustfmt --edition 2024` on the owned implementation and new test file: passed (syntax/format only).
- `git diff --check` on the owned implementation and test paths: passed.
- Local call-chain/diff review: Skills uses the existing reload owner; workflow discovery behavior retained; no Cargo/manifest/lock edits made.

Pending: compile, all 8 new runtime tests, the 3 named existing lifecycle tests, parent independent review and cross-entry milestone. Native file-event behavior remains platform-specific; this fixture does not establish Windows behavior from a macOS run. The parent owns task checkbox updates, strict validation, main docs/spec/backlog synchronization and archival.

## Independent review #2 follow-up: deduplication requires a registered owner

The parent confirmed this production defect: initial registration for startup paths A can return None after the real config consumer has accepted A → B → A. The pending latest value A was then compared with startup `active_paths` A and skipped, although no actual native owner existed. `run_skills_watcher` now deduplicates only when `watcher.is_some() && paths == active_paths`. The change still waits on a watch-channel update; None alone does not start a retry loop or trigger retries without additional configuration updates.

One new enabled exact test is source-ready:

`acp::spawn::tests::skills_hot_reload::initial_failure_retries_latest_paths_after_config_round_trip`

Expected selection: **1 test, 0 ignored** (the module now contains 9 tests). A is the startup empty custom-path list; B is an external custom root. Before releasing initial registration with injected None, the test waits for the resident Session's slash advertisement to include B's skill, then exclude it after the real config file returns to A. It requires the second A registration to succeed, excludes an intermediate B registration, then creates a user skill and checks the same Session's slash advertisement and actual model-requested `load_skill` result. The existing gate/event helpers are reused unchanged.

Scope for this follow-up is the one production condition, this one regression, and this review note. No Cargo commands were run for this follow-up. `rustfmt --edition 2024` and scoped `git diff --check` passed; these are source checks, not evidence that the runtime test passed.

Parent-reported prior batch evidence: 17 selected Pager tests produced 12 passes and 5 failures, recorded at `/tmp/grow-262-pager-final-tests.log`. The 5 prior suite failures involve missing Active event timeouts followed by mutex poisoning and are explicitly assigned to a later handoff. This new test uses the same registration helper and may be affected; it is unexecuted and no green result is claimed. Shell independent-review #1 and those 5 failures are outside this follow-up's write scope.

## Native registration investigation: shared owner handoff

The parent requested one old-binary reproduction of `acp::spawn::tests::skills_hot_reload::custom_root_creation_modification_and_removal_follow_config` using `/Users/lordcasser/workspace/projects/grow/target/debug/deps/pager-35c54ffc752122f8` (before the independent-review #2 fix). The launcher ran in canonical empty cwd `/private/tmp/grow-ac06-native-cwd-1c7_grwo`; the existing exact child helper retained its isolated home setup. Launcher PID 92816 created test child PID 92817, verified by direct parent PID, matching process group 92816, and exact binary path before sampling. `/usr/bin/sample 92817 2 1` was requested eight seconds after launch. No unrelated process was sampled.

Actual result: **0 passed, 1 failed**, binary exit 101, with `missing watcher event Active([]): [Started([])]`. Evidence is preserved in `/tmp/grow-ac06-native-exact.log`; the previous parent experiment in `/tmp/grow-ac06-cwd-probe.log` reached initial Active but stalled after the replacement Started. This reproduction does not establish a pass, and sample time is not used as an acceptance budget. The diagnostic wrapper exited normally after collecting the failed result; a subsequent exact-PID `ps` check confirmed both launcher and child were gone. No Cargo command was run.

Native sample `/tmp/grow-ac06-native-child-92817.sample.txt` locates the blocking boundary:

- Lines 652–671: `pager::acp::spawn::register_skills_watcher → SkillsFileWatcher::start → start_with_dirs → attach_new_refresh_dirs → AccessFilteredWatcher::watch → FsEventWatcher::watch_inner → run → Receiver::recv`.
- Lines 700–718 and 764–782: two `notify-rs fsevents loop` threads are inside `FSEventStreamStart → register_with_server → f2d_register_rpc → mach_msg2_trap`.
- Lines 557–575: the Session's `ProjectDiscoveryWatcher::start` also waits in the same `FsEventWatcher::run` receive path.

Locked notify 8.2.0 source confirms each `watch_inner` stops the old loop, appends one path and runs a new stream; `run` waits until its native thread finishes `FSEventStreamStart` and sends the runloop handle. The observed wait is native registration, not the Pager test gate, a Tokio mutex, or simply cwd isolation. The sample does not establish how many prior native stream starts completed or prove that batching alone would remove the stall.

The parent accepted the diagnosis and explicitly narrowed this task to an evidence handoff. Pager source/tests are released unchanged by this investigation; there is no new regression, timeout extension, production instrumentation, Shell/watcher/dependency edit, or claimed production fix. Shared ownership belongs to Shell `config::watcher::AccessFilteredWatcher / SkillsFileWatcher`. `paths_mut` batching remains an unproven candidate: the next shared-owner task must verify locked-version semantics and compare the same registration set under the unchanged twelve-second assertion, including actual completion and native start count, before applying it. The existing nine reload tests, two MCP lifecycle tests and prior failure records remain for that owner's eventual targeted validation.
