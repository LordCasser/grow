# AC06 leader verification

## Final parent milestone

最终默认2.6.2 CLI SHA `9f7585d55f2a387bbebfd68cd16dbe59c5dfac0658df6fe8372a346fb8551472` 下，`/tmp/grow-262-leader-causal-final.log` 编译成功，真实 `leader_config_file_refreshes_catalog_slash_and_custom_root_events` **1 passed / 0 failed / 0 ignored**，28.46s。使用 `--ignored --exact` 显式运行这个依赖 fresh binary 的场景。

实际覆盖配置文件-only disabled/ignore、会话插件快照、slash metadata、模型 load_skill 资格、添加custom root后的创建/修改/新技能，以及移除后修改旧skill/新增skill均不能恢复slash或模型读取。仍有效目录的新skill广告是事件处理屏障，具体leader fixture.close真实完成。

旧启动参数、canonical identity与flat slash name、全局reload counter过强假设的失败均保留。冻结契约允许旧queued事件产生幂等reload；最后测试没有静默生产日志、延时或放宽阈值，owner释放由既有四项replacement-barrier生命周期回归验证。completed registration取消修正两时序亦实际通过，见 `/tmp/grow-262-shell-final-lib.log`。

## Scope and source state

Leader-only implementation in `crates/codegen/shell/src/agent/app.rs`, its private `app/leader_skills_tests.rs`, and `tests/test_leader_skills_config_reload.rs`. Existing dirty files outside this set were preserved. No Cargo command, manifest/lock write, Pager spawn, shared SkillsFileWatcher API, Session reload owner, SDK, main spec, docs, backlog, archive or commit was performed by this coder.

`ConfigUpdate::Skills` always injects the existing `_grow/internal/reload_skills` request, including disabled/ignore changes with identical paths. Its latest paths reach the leader's existing discovery-owner task via a watch channel. Registration and old-owner teardown execute in the blocking pool; failed construction retains the prior registration and logs a warning. An initial `None` does not terminate the owner. Pending paths survive initial/replacement registration. The per-session reload owner still reads fresh effective config and its own plugin snapshot.

## Checks performed

- Loaded software-engineering, Rust principles and handoff contract; read root AGENTS/OpenSpec index and the frozen proposal/design/delta/tasks. Scoped source inspection confirmed the existing ConfigReloader → leader config consumer → internal reload → per-session SkillManager/slash chain.
- `rustfmt --edition 2024 --config skip_children=true crates/codegen/shell/src/agent/app.rs crates/codegen/shell/src/agent/app/leader_skills_tests.rs`: exit 0.
- `rustfmt --edition 2024 crates/codegen/shell/tests/test_leader_skills_config_reload.rs`: exit 0.
- `git diff --check -- crates/codegen/shell/src/agent/app.rs crates/codegen/shell/src/agent/app/leader_skills_tests.rs crates/codegen/shell/tests/test_leader_skills_config_reload.rs`: exit 0. This checks tracked diffs; rustfmt parsed both new untracked Rust files.
- Static test inventory: four new private owner tests, one existing registration-owner regression, and one ignored real-leader integration test. These are source counts, not executed test counts.

## Pending parent Cargo window

Cargo is reserved by the parent's serialized validation window. This coder has not compiled the fixture correction; runtime evidence below distinguishes the pre-correction fixture failure from the canonical-path diagnostic pass. The corrected source still requires the parent's rebuilt binary and default-TMPDIR rerun.

```sh
cargo test -p shell --lib agent::app::leader_skills_tests:: -- --nocapture
cargo test -p shell --lib agent::app::tests::blocked_watcher_registration_yields_and_allows_agent_exit -- --exact --nocapture
cargo build -p cli --bin grow
GROW_BINARY="$PWD/target/debug/grow" cargo test -p shell --test test_leader_skills_config_reload -- --ignored --nocapture
```

The first command should select 4 tests; the exact existing regression should select 1; the integration command should select 1. If the parent uses a different target directory/build profile, point GROW_BINARY at that freshly built executable. Do not accept zero selected tests or a stale binary as AC06 evidence.

## Runtime diagnosis and fixture correction

The parent's first execution of the four new lifecycle tests produced **0 passed, 4 failed**, all at `leader_skills_tests.rs:27` waiting for the first native ACP event (62.24 seconds in the raw test summary). Original log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b472-6a7c-79c0-8263-bc7b4da5b5f5/terminal/call_98a8e5036d604320bc6d56b2.log`.

Scoped diagnosis confirmed:

- `plan_skills_watch_targets` treats fixture roots `a/b/c` as recursive custom skill roots, not Grow config roots. A direct child `SKILL.md` is valid, and `/unused-grow-home` is only a classification sentinel.
- `SkillsFileWatcher` uses `notify::RecommendedWatcher` and the mini debouncer's own native/thread callbacks; this chain does not call `fsnotify::shared` or require `fsnotify::set_runtime_handle`.
- The fixture returned the uncanonicalized tempfile spelling. On macOS `/var` and `/tmp` are aliases of `/private/var` and `/private/tmp`. Native FSEvents paths use the resolved spelling, while `watcher.rs` classifies custom-root events with `event.path.starts_with(root)`. An alias spelling therefore filters out valid events. Existing native watcher tests canonicalize their temp root before registration.
- Registration acknowledgements already follow `register` returning (except the intentional blocked-registration signal). The receiver queues native events until the owner adopts the registration; this does not require a sleep or longer timeout.

With parent authorization, the same existing binary was executed without changing source or compiling:

```sh
env TMPDIR=/private/tmp RUST_MIN_STACK=16777216 \
/Users/lordcasser/workspace/projects/grow/target/debug/deps/shell-34fc88a27b856cb5 \
agent::app::leader_skills_tests:: --nocapture --test-threads=1
```

Result: **4 passed, 0 failed, 0 ignored**, 4784 filtered out, **32.06 seconds**, exit 0. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b480-6f61-7793-aff6-c1a1b8586ec4/terminal/call_2373910c21124634aafb8851f95301fd.log`. Changing only the temporary path spelling establishes the fixture alias cause; it does not establish that rebuilt corrected source has passed.

During the authorized short write window, only the test `root()` helper changed: after creating the directory it now calls `dunce::canonicalize` before returning its string. All four tests, original timeouts and assertions remain. `rustfmt --edition 2024 --check crates/codegen/shell/src/agent/app/leader_skills_tests.rs` and the local `git diff --check` returned exit 0. No production or shared API change was needed for this failure. Source was released for the parent's unified rebuild; the rebuilt four-test run with default TMPDIR remains pending.

## Prepared coverage and validation boundaries

- Initial registration blocked while paths changes twice: adopt the latest root, receive its real native event, and observe no event from the removed root.
- Failed replacement retains the old watch, and a later valid paths update recovers.
- Initial no-watch result recovers on later valid paths.
- Cancellation during replacement returns while registration is blocked; the existing generic owner regression checks eventual detached-result disposal.
- Real LeaderFixture child and LeaderClient IPC: edit only config.toml disabled/ignore, await the existing session's slash advertisement, and drive model `load_skill` calls against mock inference to assert actual success/disabled/missing results and absent forbidden bodies. No UI toggle or synthetic ConfigUpdate supplies the reload.
- Add external custom root through config, subsequently create/edit skills there, observe model loading and updated slash metadata; remove the root, then verify subsequent creation/editing cannot restore slash visibility or model-callable eligibility. A still-active native source supplies an event/adoption barrier. Native removed-root owner retirement and absence of its events are covered separately by `leader_skills_tests::latest_paths_survive_initial_registration_and_removed_root_stops_events`, which first observes an adopted replacement-root event before checking the removed source.

## Remaining gate

Parent compilation/runtime verification and review remain required. Linux/Windows native events are not validated by this macOS workspace. During the parent's coordinated short write window, all three blocking teardown awaits were changed to reuse `finish_blocking_registration`: cancellation detaches the blocking cleanup without reporting confirmed retirement, and its closure retains the actual registration until native Drop completes. The final app source was reparsed by rustfmt and `git diff --check` returned exit 0 before source release.

## Final-review regression: completed registration output on cancellation

Final review identified a cancellation race in `finish_blocking_registration`: with a completed blocking task and a ready cancellation token, biased selection chooses cancellation, and dropping its JoinHandle can synchronously destroy its still-owned output on the LocalSet. Native watcher teardown can therefore block the async owner even though registration originally ran in the blocking pool.

The bounded follow-up changes only `agent/app.rs`: cancellation now schedules `spawn_blocking(move || drop(registration))` and immediately returns `None`. Both unfinished and completed handles are retired away from the LocalSet; no native teardown completion is claimed or awaited, and the handle/task retains its eventual output until it is destroyed. This implements the existing frozen native-Drop lifecycle contract without a new API or delta.

Added `agent::app::tests::completed_watcher_registration_cancellation_drops_output_off_local_set`. It explicitly observes `registration.is_finished()` before cancelling, records the output Drop thread, asserts it differs from the LocalSet thread, blocks that Drop while another LocalSet actor completes, and releases/observes the final owner. Its Drop wait is bounded so the old synchronous-drop regression fails instead of hanging the process. The existing cancel-before-finish test remains unchanged.

Executed after the parent's SOURCE WRITE RELEASE:

```sh
rustfmt --edition 2024 --config skip_children=true crates/codegen/shell/src/agent/app.rs
git diff --check -- crates/codegen/shell/src/agent/app.rs
```

Both returned exit 0. Source was immediately released for the parent's unified Shell compilation. This follow-up did not run Cargo, change Pager/other reload logic, modify manifest/lock, or disturb existing dirty work. Exact pending checks (one selected test each):

```sh
cargo test -p shell --lib agent::app::tests::completed_watcher_registration_cancellation_drops_output_off_local_set -- --exact --nocapture
cargo test -p shell --lib agent::app::tests::blocked_watcher_registration_yields_and_allows_agent_exit -- --exact --nocapture
```

Compilation, executed counts and runtime results for this final-review fix remain pending the parent's serialized window; static parsing is not a runtime pass.

### Fixture CLI args follow-up (2026-11-07)

- 父会话实测：`target/debug/grow agent leader --relay-on-demand --help` 以 exit 2 报 unexpected argument；新 CLI 的 leader 子命令仅接受 `--no-exit-on-disconnect`、`--no-auto-update` 等，`--relay-on-demand` 已 obsolete。
- 影响：`test-support/src/leader.rs` 的 `start_with_binary_base_url_timeout` 以旧参数启动真实 leader，导致 readiness 30s 失败（而非 product reload 缺陷）。
- 修复：仅从该 args 数组删除 `--relay-on-demand`（leader.rs:213），其余启动参数、no-exit-on-disconnect / no-auto-update、env、socket、owned-child cleanup 原样保留；未引入兼容 flag 或生产别名。
- `rustfmt --edition 2024` 通过；`git diff` 局部核对仅删 1 行。运行时验证由父在 leader integration 用例中确认。

### Leader integration slash identity correction

The parent ran the real leader integration with the fresh 2.6.2 CLI (reported SHA prefix `9f7585`). `/tmp/grow-262-leader-final.log` records **0 passed, 1 failed**: startup and `session/new` succeeded, then the initial slash expectation timed out. The captured advertisement contains `name = ac06-shared`, `_meta.plugin_name = ac06`, and `_meta["grow/commandSource"] = skill`; the fixture had compared the flat wire name directly with `ac06:ac06-shared`.

Scoped source checks confirm `slash_commands::available_commands` advertises plugin ownership separately from the command name, `resolve_catalog_skill` accepts `format_skill_name` (`plugin:name`) for model calls, and config discovery checks disabled entries against `dedup_key` (`plugin:name`). Only `Client::commands` in `test_leader_skills_config_reload.rs` changed: expected identities are matched against the flat name plus exact plugin metadata, requiring command source `skill`. Native identities also require absent plugin metadata. All disabled/ignore assertions, canonical model-call inputs, plugin snapshot checks, custom-root behavior and timeouts remain intact.

Executed `rustfmt --edition 2024`, its `--check` form, and `git diff --check -- crates/codegen/shell/tests/test_leader_skills_config_reload.rs`: all exit 0. The fixture remains untracked, so git's diff check does not inspect its contents; rustfmt parsed and checked the corrected file. No Cargo or product-source edit was performed. **SOURCE READY / RELEASED** for the parent's exact integration rerun; this correction has no runtime pass yet.

### Leader integration removed-root observation correction

The parent's next real integration run, `/tmp/grow-262-leader-identity-final.log`, records **0 passed, 1 failed**, in 32.72 seconds. It reached the last assertion after all preceding config-only disabled/ignore, plugin model loads, external-root creation/editing and removal advertisement checks completed. The failure compared the global `Reloaded skills from disk` count: actual 8 versus expected 7. That counter assumption is stronger than the frozen design, which allows already queued old-source events to cause an idempotent reload; generic config/reload work can also be pending. This failure does not establish a retained removed-root native owner.

Only the final removal observation in the integration fixture changed. After removal, it creates a new skill and edits a previously present skill under the removed root, then creates a skill under the still-active native root. Receipt of that active-root slash advertisement supplies a real event/adoption barrier. The removed identities must remain absent from slash advertisement, and actual model `load_skill` calls for the new identity and both former identities must return `not found` without their new/old body markers. A final slash assertion remains after those calls. The two fixed sleeps and global-counter equality were replaced, without extending a timeout or allowing a counter threshold. All earlier assertions and the real `fixture.close().await` cleanup remain.

This product milestone checks catalog visibility and model eligibility after removal; it does not claim no reload occurs. Native owner retirement/no removed-source events remain the separate lifecycle evidence from the four owner tests, specifically `latest_paths_survive_initial_registration_and_removed_root_stops_events`: an actual adopted replacement-root event forms its barrier before a removed-source write is checked.

Executed `rustfmt --edition 2024`, its `--check` form, and `git diff --check -- crates/codegen/shell/tests/test_leader_skills_config_reload.rs`: exit 0 for each. The untracked fixture is parsed/checked by rustfmt; git diff check alone does not inspect untracked content. No Cargo or production-source edits were performed. **SOURCE READY / RELEASED**; the parent must run the exact integration again to establish a pass for this corrected observation.


