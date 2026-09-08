## ADDED Requirements

### Requirement: Timing and timestamp macros
timed 宏 SHALL 保留代码块返回值，裸调用返回 value 与毫秒耗时；log 分支在指定或默认 debug 级别记录时间，sync/async try 分支保留 Result 并为 Err 附加错误字段。

#### Scenario: 时间戳打印
- **WHEN** 调用 tprintln 或 teprintln
- **THEN** 分别通过 tracing info/warn 输出 Unix 秒前缀；系统时间早于 epoch 时用 0，不直接占用 stdout。

#### Scenario: 调用方集成
- **WHEN** 使用包含日志的宏
- **THEN** 宏展开依赖调用方可解析的 tracing 路径；crate 本身没有运行时 dependency。

证据：`crates/codegen/tracing-macros/src/timed.rs` — `timed`；`crates/codegen/tracing-macros/src/timestamp.rs` — `tprintln`。

### Requirement: Hermetic git test execution
测试 Git 辅助 SHALL 可通过 GIT_BIN_PATH 一次性前置 binary 目录；run_git_with_env 固定作者身份、屏蔽 global/system 配置、禁止交互并断言命令成功。

#### Scenario: 调用方覆盖环境
- **WHEN** run_git_with_env 提供 envs
- **THEN** envs 最后应用，可覆盖辅助函数默认值；返回 trim 后 stdout。

#### Scenario: 初始化和提交辅助
- **WHEN** 调用 init_git_repo 或 git_commit_all
- **THEN** 执行 init/config 或 add/commit；这些便捷入口只 unwrap 进程启动结果，不额外断言 exit status。

证据：`crates/common/test-utils/src/git.rs` — `run_git_with_env`；`crates/common/test-utils/src/git.rs` — `ensure_hermetic_git_on_path`。

### Requirement: Runfiles and test fixtures
启用 bazel feature 时测试资源 SHALL 按 RUNFILES_DIR/TEST_SRCDIR 和 manifest 查找；crate_root 宏找不到时回退调用方 CARGO_MANIFEST_DIR。

#### Scenario: 关闭 bazel
- **WHEN** 调用 try_resolve_runfiles
- **THEN** 返回 None；default-bazel feature 启用 bazel。

#### Scenario: 生成测试数据
- **WHEN** 调用 env_usize、ico_with_png_frame、write_fanout_tree 或 make_feature_branch
- **THEN** 分别提供解析失败默认值、单帧 ICO 包装、按目录分组的约量文件树、可供 rebase 的 feature/base 提交布局。

证据：`crates/common/test-utils/src/runfiles_util.rs` — `crate_root`；`crates/common/test-utils/src/env.rs` — `env_usize`；`crates/common/test-utils/src/image.rs` — `ico_with_png_frame`；`crates/common/test-utils/src/git.rs` — `write_fanout_tree`。

### Requirement: Scoped tracing counters
测试日志计数器 SHALL 根据 message 前缀累计事件，clone 共享计数；提供 thread scoped 与 process global 安装。

#### Scenario: 查询未注册前缀
- **WHEN** 调用 count 时前缀不在注册表
- **THEN** panic，不把错误测试配置当成零计数。

#### Scenario: 安装边界
- **WHEN** 使用 thread scoped 或 global 安装
- **THEN** thread scoped 只观察当前线程；global 已存在 subscriber 时 panic，可选将过滤日志同时输出到 stderr。

证据：`crates/common/test-utils/src/tracing_capture.rs` — `MessagePrefixCounter`。

### Requirement: CPU profiler lifecycle
CpuProfileManager SHALL 从 Inactive 启动为 Active，take_stop_handle 转为 Stopping 并发布 watch=false，complete_stop 清除 Stopping 并发布 true；重复启动、无 active 停止、停止中操作返回对应 ControlErrorCode。

#### Scenario: 分离停止
- **WHEN** 调用 stop_handle.finish
- **THEN** 消耗 engine 并写产物，manager 状态仍由调用者 complete_stop 收尾；同步 stop 无论成功失败都执行 complete_stop。

#### Scenario: 关机
- **WHEN** finalize_on_shutdown 遇到 active、stopping 或 inactive
- **THEN** active 同步停止并返回 Some；其余返回 None，不等待其他 caller 持有的 stop handle；需要协调的 host 订阅 completion。

#### Scenario: 配置和平台
- **WHEN** 启动生产 profile
- **THEN** Unix 支持，非 Unix 返回 unsupported；频率默认 1000 Hz，范围 1..4000，失败附带 provided/min/max。

证据：`crates/codegen/shell-base/src/cpu_profile.rs` — `CpuProfileManager`；`crates/codegen/shell-base/src/cpu_profile.rs` — `CpuProfileStopHandle`；`crates/codegen/shell-base/src/cpu_profile.rs` — `validate_frequency`。

### Requirement: CPU profile artifact generation
CPU profile SHALL 默认输出 grow_home/profiles 下带 PID 和 UTC 微秒时间的 folded 文件；显式 folded/txt 后缀按原路径，目录用 leader 前缀，其他路径用文件名作为前缀，最多尝试 32 个候选。

#### Scenario: 碰撞
- **WHEN** 开始时目标存在或停止时目标被其他 writer 创建
- **THEN** 开始返回 OutputPathCollision；真正创建在 stop 使用 create_new，再次拒绝覆盖，选路径并不提前预留文件。

#### Scenario: 采样产物
- **WHEN** Unix pprof 停止并构建报告
- **THEN** 按 thread 与反向 frames/symbols 形成 folded stack 计数行并排序，非空末尾加换行；屏蔽 libc/libgcc/pthread/vdso，报告/写入失败分类为 InternalError/ArtifactWriteFailed，不在本包渲染 flamegraph。

证据：`crates/codegen/shell-base/src/cpu_profile.rs` — `derive_output_path`；`crates/codegen/shell-base/src/cpu_profile.rs` — `derive_unique_artifact_path`；`crates/codegen/shell-base/src/cpu_profile.rs` — `folded_stacks`。

### Requirement: Shell test seams
test-support feature SHALL 暴露 EnvVarGuard 和 CPU profiler 测试注入，default-bazel 启用 test-support；EnvVarGuard 持有共用锁直至 drop，恢复原 UTF-8 环境值或删除变量。

#### Scenario: 测试能力
- **WHEN** 强制 unsupported 或注入 fake engine
- **THEN** 可独立覆盖平台能力/停止结果，注入启动仍校验频率和路径；锁只协调使用该 guard 的调用，不控制其他环境读写，非 UTF-8 原值按缺失处理。

证据：`crates/codegen/shell-base/src/env.rs` — `EnvVarGuard`；`crates/codegen/shell-base/src/cpu_profile.rs` — `start_with_engine_for_test`；`crates/codegen/shell-base/src/cpu_profile.rs` — `force_unsupported_for_test`。

### Requirement: Shell utility formatting and reexports
shell-base SHALL 提供 Unicode 字符截断、基于 RandomState 哈希的 53 位 [0,1) 采样数、rate 比较和 OS 内核版本辅助函数，并重新导出 config 路径与 client-support clipboard/stderr 能力。

#### Scenario: 字符串和概率
- **WHEN** truncate 或 probabilistic_sample
- **THEN** 截断按字符边界不加省略号；采样比较 random<rate，无额外 clamp，非密码学保证，NaN 比较为 false。

#### Scenario: 系统版本
- **WHEN** Unix uname 或 Windows cmd /C ver 成功
- **THEN** Unix 返回小写 kernel 加 release，Windows 提取 [Version ...] 为 windows 版本；失败回退 std::env::consts::OS。本包重导出不另实现 home、clipboard 或 stderr 锁。

证据：`crates/codegen/shell-base/src/util/mod.rs` — `truncate`；`crates/codegen/shell-base/src/util/mod.rs` — `random_f64`；`crates/codegen/shell-base/src/util/uname.rs` — `os_kernel_and_release`；`crates/codegen/shell-base/src/util/grow_home.rs` — `grow_home`。

### Requirement: Markdown fuzz package boundary
markdown-fuzz SHALL 作为独立 nested workspace、不可 publish 的 edition 2021 包提供 libFuzzer render_all target，引用父 markdown；target 禁用 Cargo test/doc/bench。

#### Scenario: 普通 workspace test
- **WHEN** 运行根 workspace 的测试
- **THEN** 不会据此运行这个独立 fuzz target。

#### Scenario: 默认路径
- **WHEN** render_all 接到合法 UTF-8
- **THEN** 仅执行 pretty true/false × full/streaming 四条 ratatui 路径，Syntect 恒 None。

证据：`crates/codegen/markdown/fuzz/Cargo.toml` — `cargo-fuzz`；`crates/codegen/markdown/fuzz/fuzz_targets/render_all.rs` — `fuzz_target!`。

### Requirement: Markdown fuzz chunking and oracle
render_all SHALL 拒绝非 UTF-8 输入；streaming 轮换 1/16/32 字节目标长度并向后对齐字符边界，每块 push_and_render。

#### Scenario: 多字节输入
- **WHEN** 切点落在 UTF-8 字符内部
- **THEN** 将 end 向后推进至合法边界，不按固定字符数分块。

#### Scenario: 测试判定边界
- **WHEN** 一个输入执行结束
- **THEN** 不调用 finish、不比较 full/streaming 输出、不覆盖 ANSI 或 Syntect；README 的八组合描述不是实际覆盖。

证据：`crates/codegen/markdown/fuzz/fuzz_targets/render_all.rs` — `CHUNK_SIZES`。

### Requirement: Markdown fuzz seed corpus
fuzz seeds SHALL 保留 9 份手工文本，覆盖 code、inline、list、mixed nesting、table、math、Unicode、thematic emoji 和较长 benchmark 文本。

#### Scenario: 使用 benchmark seed
- **WHEN** fuzzer 将 bench.md 作为输入
- **THEN** 其中架构与性能陈述只是被渲染文本，不是仓库事实证据。

#### Scenario: 执行范围
- **WHEN** 仅完成本次源码审阅和 markdown 单测
- **THEN** 没有执行 cargo-fuzz campaign，不能声称 corpus 已被 libFuzzer 跑过。

证据：`crates/codegen/markdown/fuzz/seeds/render_all/bench.md` — `Architecture Overview`；`crates/codegen/markdown/fuzz/seeds/render_all/math.md` — `Math seed`；`crates/codegen/markdown/fuzz/README.md` — `seeds/`。
### Requirement: Pager package feature topology

pager SHALL 默认启用jemalloc与sandbox-enforce；本crate的jemalloc与release-dist为空feature，sandbox-enforce只转发sandbox/enforce。default-bazel在默认两项外增加test-support，test-support只转发pager-render/test-support。Mermaid依赖始终参与编译，其显示由运行时设置控制。

#### Scenario: Default Cargo feature set
- **WHEN** 按pager默认features构建
- **THEN** 解析jemalloc与sandbox-enforce，其中只有后者向sandbox依赖转发enforce。

#### Scenario: Bazel support feature set
- **WHEN** 启用default-bazel
- **THEN** 除默认能力外开放pager-render的test-support。

源码证据：`crates/codegen/pager/Cargo.toml` — `[features] / mermaid dependency`。


### Requirement: Pager build provenance string

pager build script SHALL 在.git/HEAD或GROW_VERSION变化时重跑；commit取git rev-parse --short HEAD成功且UTF-8的trim结果，否则unknown。version优先GROW_VERSION，其次CARGO_PKG_VERSION，最后0.0.0，并导出VERSION_WITH_COMMIT为`version (commit)`。

#### Scenario: Git metadata unavailable
- **WHEN** git命令失败、状态非成功或stdout非UTF-8
- **THEN** 编译环境中的commit部分为unknown，版本仍按环境变量优先级解析。

源码证据：`crates/codegen/pager/build.rs` — `fn main`。

### Requirement: Pager playground terminal harness lifecycle
The six src/bin playgrounds SHALL be standalone Crossterm executables for manual visual inspection: each enables raw mode, enters the alternate screen, creates a Ratatui terminal, polls/draws in a loop and restores the screen/raw mode after a normal exit. Mouse playgrounds additionally enable then disable capture. Cleanup is procedural rather than guarded; any draw/poll/read/execute error propagated by ? before the tail cleanup can leave terminal state enabled. These binaries contain no assertions and do not count as automated validation merely by compiling.

#### Scenario: Normal exit
- **WHEN** a configured quit key ends a playground loop
- **THEN** the tail path leaves alternate screen and disables raw mode.

#### Scenario: Loop IO failure
- **WHEN** draw or event polling returns an error
- **THEN** the binary returns early and this code does not prove terminal restoration.

#### Scenario: Static audit
- **WHEN** the source is read but no binary is run
- **THEN** no visual behavior or real terminal interaction is claimed verified.

证据：`crates/codegen/pager/src/bin/*.rs` — `main`。

### Requirement: Pager Mermaid scrollback playground
mermaid-playground SHALL choose one of five built-in prompt/answer samples by parsing MERMAID_SAMPLE as usize, defaulting zero and applying modulo five. It inserts a user and agent block into real ScrollbackState, prepares production layout, initially scrolls entry zero to top, renders through ScrollbackPane with ScratchBuffer, and copies the intermediate buffer cell-by-cell. Up/Down move two lines, PageUp/PageDown fifteen, and q, Esc or Ctrl-C exit; the documented Ctrl-Q label is not implemented.

#### Scenario: Out of range sample
- **WHEN** MERMAID_SAMPLE parses to 7
- **THEN** sample 2 is selected by modulo.

#### Scenario: Invalid sample
- **WHEN** MERMAID_SAMPLE is absent or non-numeric
- **THEN** sample zero is selected.

#### Scenario: Page navigation
- **WHEN** PageDown is read
- **THEN** scrollback moves down 15 lines before the next draw.

证据：`crates/codegen/pager/src/bin/mermaid_playground.rs` — `SAMPLES / main`。

### Requirement: Pager raw mouse event playground
mouse-events-playground SHALL capture Crossterm mouse events and render the newest raw event, sample selectable text and a newest-first log capped at 200 entries. ScrollUp/ScrollDown update last_scroll_at and a rolling last-20 interval average; the first scroll clears intervals, and a non-scroll mouse event clears interval history only when a prior scroll exists without clearing last_scroll_at. Keyboard and other events are logged. Esc or Ctrl-Q exits.

#### Scenario: Twenty first interval
- **WHEN** more than 21 scroll events arrive
- **THEN** only the newest 20 inter-scroll intervals contribute to avg.

#### Scenario: Non-scroll event
- **WHEN** Moved follows a scroll
- **THEN** interval history is cleared and the event is still logged.

#### Scenario: Log capacity
- **WHEN** over 200 messages are pushed
- **THEN** oldest messages are removed from the back.

证据：`crates/codegen/pager/src/bin/mouse_events_playground.rs` — `App::push / main / draw`。

### Requirement: Pager question and todo view playground scenarios
question-view-playground SHALL expose four hard-coded QuestionView scenarios covering previews, multi-select and three tabs; j/k or arrows clamp cursor through QuestionViewState, Space/Enter toggles the current option, Tab/BackTab changes question, and n/p wraps scenarios. todo-pane-playground SHALL expose five TodoStatus mixtures, reset each scenario to show completed rows, forward remaining keys to TodoPane, display live counts and allow h plus n/p wrapping. Both use production render/height APIs but do not submit answers, persist todos or assert rendered cells.

#### Scenario: Question scenario switch
- **WHEN** n is pressed on the last scenario
- **THEN** the first scenario is reconstructed with empty selection state.

#### Scenario: Todo hide completed
- **WHEN** h is forwarded in the cancelled-plus-completed scenario
- **THEN** the pane computes its production empty/count placeholder for manual inspection.

#### Scenario: Playground selection
- **WHEN** Space or Enter is pressed in question view
- **THEN** only local QuestionViewState selection changes; no tool response is sent.

证据：`crates/codegen/pager/src/bin/question_view_playground.rs`；`crates/codegen/pager/src/bin/todo_pane_playground.rs`。

### Requirement: Pager scrollback search playground
scrollback-search-playground SHALL build four sample blocks and an open ScrollbackSearchState, poll background matching each loop, reveal the current result through entry ID and line, render the real pane with regex highlighting, and reserve one bottom row for the shared search bar plus a right-aligned match/error counter. While composing, keys update the query; Enter accepts browsing, n/N navigate only after acceptance, Esc resets a nonempty query or exits when empty, and Ctrl-Q exits. The newest-first event log caps at 200.

#### Scenario: Async result
- **WHEN** poll reports a changed match set
- **THEN** the current match line is revealed before drawing.

#### Scenario: Escape with query
- **WHEN** Esc is pressed while query is nonempty
- **THEN** search state is replaced by a newly open empty state without exiting.

#### Scenario: Invalid regex
- **WHEN** the query has an error and no current index
- **THEN** the counter displays bad pattern.

证据：`crates/codegen/pager/src/bin/scrollback_search_playground.rs` — `App / handle_key / draw / main`。

### Requirement: Pager scrollback selection playground
scrollback-selection-playground SHALL render sample blocks through ScrollbackPane, retain the last frame's ResolvedSelectionModel, display exact/nearest hit diagnostics, and overlay a local PersistentTextSelection. Clicks on the same entry/range/block line within strictly 300ms count up to triple then reset. Double-click selects a URL range first or configured word boundaries; triple-click selects the full selectable line. Highlight holds indefinitely when keep_text_selection holds, otherwise expires after 500ms. Left-down clears selection, Esc clears it before exiting, and displayed selected text is UTF-8-boundary truncated near 60 bytes. This playground does not implement drag updates or clipboard copying.

#### Scenario: URL double click
- **WHEN** the second click hits a URL column within 300ms
- **THEN** the URL range becomes a DoubleClick persistent selection.

#### Scenario: Triple click
- **WHEN** a third matching click hits a nonempty selectable line
- **THEN** the whole line becomes selected and click count resets.

#### Scenario: Flash mode
- **WHEN** keep_text_selection does not hold and 500ms elapse
- **THEN** the local highlight is cleared.

证据：`crates/codegen/pager/src/bin/scrollback_selection_playground.rs` — `selection_timeout_ms / App::count_click / handle_click / select_word / select_line / main / draw`。

### Requirement: Pager edit highlight Criterion matrix
edit_highlight bench SHALL compare production hunk-only render, file-scoped style compute, precomputed paint and compute-plus-paint over generated 500-line/8-hunk and 10,000-line/40-hunk Python fixtures. The non-product prefix-per-hunk baseline runs only on the small fixture. Setup/precomputation/logging are excluded where stated; width is 120, inputs remain below 2MiB/50k lines, and no timing threshold is asserted.

#### Scenario: Precomputed paint
- **WHEN** paint_with_precomputed is measured
- **THEN** style computation occurred outside its timed closure.

#### Scenario: Heavy matrix
- **WHEN** the 10k fixture runs
- **THEN** the non-product prefix baseline is omitted.

#### Scenario: Reported duration
- **WHEN** Criterion emits a result
- **THEN** this source does not classify it pass or fail.

证据：`crates/codegen/pager/benches/edit_highlight.rs`。

### Requirement: Pager render and reveal Criterion workloads
render bench SHALL use a prebuilt two-entry rich-Markdown corpus for single/full-list frames, a 3000-entry expanded corpus for production paint_window paging, and clean-cache versus deliberately dirtied reveal paths. Parsing, wrap priming, corpus creation and layout settlement are setup. Viewport is 120x50 and scroll steps are 10 lines; no numeric gate is asserted.

#### Scenario: Single frame
- **WHEN** render/single_frame iterates
- **THEN** a reset reused buffer renders offset zero with primed caches.

#### Scenario: Windowed scroll
- **WHEN** windowed_scroll iterates
- **THEN** each offset renders only the paint_window slice.

#### Scenario: Forced rebuild
- **WHEN** navigate_rebuild iterates
- **THEN** one thinking character dirties layout before reveal.

证据：`crates/codegen/pager/benches/render.rs`。

### Requirement: Pager resize Criterion workloads
resize bench SHALL synthesize 400 turns of eight RenderBlock kinds, settle 120x50 layout, then measure alternating 120/119 widths, a 120-through-101 drag plus restoration, and repeated same-width preparation. Fixture prose is payload rather than implementation evidence. It sends no terminal Resize event and asserts no latency/allocation threshold.

#### Scenario: Width step
- **WHEN** two width_step iterations run
- **THEN** widths alternate 119 then 120.

#### Scenario: Drag
- **WHEN** drag_20_steps runs once
- **THEN** 20 descending widths and the restored 120 width are prepared.

#### Scenario: Same width
- **WHEN** same_width_noop iterates
- **THEN** settled 120x50 is submitted again without terminal IO.

证据：`crates/codegen/pager/benches/resize.rs`。

### Requirement: Pager search Criterion workloads
search bench SHALL synthesize 30,000 prompts and separately measure worst-case regex index scan, steady update_query plus one opportunistic poll after corpus shipment, and cold update_query on a fresh state including initial corpus projection. Each uses 10 samples and one-second warmup. Steady does not await a particular result, cold omits poll, and no duration is an acceptance gate.

#### Scenario: Raw scan
- **WHEN** scan iterates with fox
- **THEN** the synced index collects a match in every entry.

#### Scenario: Steady query
- **WHEN** content is unchanged
- **THEN** one query is enqueued and poll called once without blocking.

#### Scenario: Cold query
- **WHEN** a fresh search state is used
- **THEN** update includes initial corpus shipping but not result delivery.

证据：`crates/codegen/pager/benches/search.rs`。

### Requirement: Shell Cargo.toml feature and entrypoint contract

Cargo.toml SHALL define the shell package as edition/version workspace metadata with default=[], unstable=[], dhat-heap=dep:dhat, test-support=[], and default-bazel=test-support; required-feature gates SHALL remain attached to the fork_copy bench and the five test targets declared in the manifest, while session_list remains ungated. Unix/windows dependency sections and dev-dependencies are part of the static crate boundary; no dependency resolution is inferred.

#### Scenario: Feature gate
- **WHEN** the manifest is read without enabling optional features
- **THEN** default remains empty and only explicitly declared feature relationships are recorded.

#### Scenario: Required test target
- **WHEN** a gated test or bench target is selected
- **THEN** the target requires test-support exactly as declared; this audit does not execute it.

证据：`crates/codegen/shell/Cargo.toml`。

### Requirement: Shell crates/codegen/shell/src/agent/mvp_agent/tests/process_scope_reclaim.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 sleeper, died, still_running, insert_session_with_scope, close_reaps_enrolled_session_child, close_is_isolated_across_sessions。源码显式使用 explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: insert_session_with_scope
- **WHEN** 执行 insert_session_with_scope 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/agent/mvp_agent/tests/process_scope_reclaim.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/mvp_agent/tests/subagent_spawn_context_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 subagent_spawn_context_inherits_parent_permission_handle, subagent_spawn_context_shares_parent_goal_loop_gate, subagent_spawn_context_inherits_parent_ask_user_question_gate, subagent_spawn_context_inherits_parent_process_scope, nested_spawn_context_uses_immediate_parent_workspace_and_route。源码显式使用 filesystem or durable record I/O、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: subagent_spawn_context_inherits_parent_permission_handle
- **WHEN** 执行 subagent_spawn_context_inherits_parent_permission_handle 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: subagent_spawn_context_inherits_parent_ask_user_question_gate
- **WHEN** 执行 subagent_spawn_context_inherits_parent_ask_user_question_gate 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: subagent_spawn_context_inherits_parent_process_scope
- **WHEN** 执行 subagent_spawn_context_inherits_parent_process_scope 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/agent/mvp_agent/tests/subagent_spawn_context_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/subagent/tests/mod.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 delegated_task_is_a_child_user_turn, resume_authority_follows_immediate_security_parent, normalized_child_seeds_its_system_head_before_timeline_creation, new_child_system_head_is_part_of_the_preserved_prefix, inherited_child_context_requires_and_preserves_its_system_head, canonical_total_tokens_does_not_double_count_reasoning, cancellation_makes_an_otherwise_complete_usage_snapshot_incomplete, usage_ack_precedes_terminal_presentation, subagent_inherits_session_cli_overrides, emit_subagent_notification_stamps_one_event_id_on_both_paths, subagent_max_turns_definition_wins_else_inherits_parent, resume_worktree_action_covers_three_outcomes, subagent_inherits_parent_lsp_via_context, no_parent_lsp_means_child_gets_none, auto_wake_test_request, admit_test_completion_receipt, background_subagent_completion_emits_one_acknowledged_durable_receipt, loop_completion_uses_the_same_acknowledged_durable_receipt (plus 66 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: inherited_child_context_requires_and_preserves_its_system_head
- **WHEN** 执行 inherited_child_context_requires_and_preserves_its_system_head 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: canonical_total_tokens_does_not_double_count_reasoning
- **WHEN** 执行 canonical_total_tokens_does_not_double_count_reasoning 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: subagent_inherits_session_cli_overrides
- **WHEN** 执行 subagent_inherits_session_cli_overrides 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/agent/subagent/tests/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/subagent/tests/rest.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 normalize_forked_context_strips_project_layout, normalize_forked_context_consecutive_users, end_to_end_normalized_conversation_shape, cached_prompt_text_is_task_not_background, last_user_message_is_task_after_normalization, subagent_worktree_snapshot_gate_defaults_off, subagent_worktree_snapshot_gate_remote_enables, subagent_worktree_snapshot_gate_local_overrides_remote, subagent_worktree_snapshot_gate_local_enables, subagent_tool_params_carry_ask_user_question_timeouts, initial_context_source_resumed_variant, resume_initial_context_preserves_head_only, resume_prefix_len_is_system_head_only, resume_prefix_len_is_zero_without_system_head, resume_source_worktree_reuse, resolve_child_cwd_uses_override_when_no_worktree, resolve_child_cwd_worktree_takes_precedence_over_override, resolve_child_cwd_falls_back_to_parent_when_no_overrides (plus 68 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: resume_initial_context_preserves_head_only
- **WHEN** 执行 resume_initial_context_preserves_head_only 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: resume_prefix_len_is_zero_without_system_head
- **WHEN** 执行 resume_prefix_len_is_zero_without_system_head 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: resumed_session_preserves_its_seeded_system_head
- **WHEN** 执行 resumed_session_preserves_its_seeded_system_head 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/agent/subagent/tests/rest.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/build_tool_parse_error_message_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 test_malformed_json_includes_original_args_and_position, test_valid_json_no_invalid_json_note, test_empty_arguments_no_extra_content, test_long_arguments_are_truncated, test_truncate_bytes_non_ascii, test_non_ascii_arguments_truncated_safely。源码显式使用 serde-backed wire/config types、explicit error/result paths、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_malformed_json_includes_original_args_and_position
- **WHEN** 执行 test_malformed_json_includes_original_args_and_position 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_valid_json_no_invalid_json_note
- **WHEN** 执行 test_valid_json_no_invalid_json_note 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_empty_arguments_no_extra_content
- **WHEN** 执行 test_empty_arguments_no_extra_content 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/build_tool_parse_error_message_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/chat_history_integrity_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 IDENTICAL_CALLS_TO_TRIP_NUDGE, TODO_ARGS, CANCEL_MARKER, STATIONARITY_NUDGE_MARKER, tool_call_sse, drain_gateway, drain_persistence, tool_results_by_call_id, mid_turn_user_injection_must_not_duplicate_tool_results_for_one_tool_use_id。源码显式使用 explicit error/result paths、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/chat_history_integrity_tests.rs
- **THEN** 记录其 IDENTICAL_CALLS_TO_TRIP_NUDGE, TODO_ARGS, CANCEL_MARKER, STATIONARITY_NUDGE_MARKER, tool_call_sse, drain_gateway, drain_persistence, tool_results_by_call_id, mid_turn_user_injection_must_not_duplicate_tool_results_for_one_tool_use_id 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/chat_history_integrity_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/client_hooks_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 install_client_hook, spawn_deny_responder, str, client_hooks_fire_without_file_registry, pre_tool_use_resolves_meta_dispatch_tool_name_end_to_end, subagent_inherits_parent_pre_tool_use_client_hook, on, pre_tool_use_first_deny_skips_later_client_without_side_effect, post_tool_use_and_failure_never_double_fire, pre_tool_use_deny_feeds_reason_back_and_continues_turn, stop_client_gate_short_circuits_after_first_deny, stop_client_gate_force_stop_short_circuits_later_context, run_stop_gate_keep_working_and_cap, file_registry_with_stop_spec, file_force_stop_skips_all_later_client_callbacks, client_force_stop_attribution_is_registration_ordered, subagent_session_gates_on_subagent_stop。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: client_hooks_fire_without_file_registry
- **WHEN** 执行 client_hooks_fire_without_file_registry 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: subagent_inherits_parent_pre_tool_use_client_hook
- **WHEN** 执行 subagent_inherits_parent_pre_tool_use_client_hook 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: pre_tool_use_first_deny_skips_later_client_without_side_effect
- **WHEN** 执行 pre_tool_use_first_deny_skips_later_client_without_side_effect 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/client_hooks_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 async_compaction_threshold_tracks_resolved_hard_threshold, async_compaction_starts_at_exact_pre_threshold_but_not_at_the_hard_threshold, async_compaction_runs_beside_foreground_and_publishes_only_at_boundary, async_compaction_promotes_the_same_provider_request, async_compaction_first_cross_turn_commit_refreshes_recall_tools, async_compaction_promoted_job_is_cancelled_by_control_transition, async_compaction_promoted_job_with_incomplete_usage_cannot_commit, async_compaction_rechecks_model_after_promotion_wait, async_compaction_cancel_discards_late_provider_result, async_compaction_goal_concurrency_is_exactly_charged, async_compaction_rejects_changed_model_route, async_compaction_failure_does_not_interrupt_or_restart_in_the_same_turn, async_compaction_goal_budget_closes_and_settles_without_a_wait_cycle, async_compaction_authority_transition_cancels_before_publication, async_compaction_promotion_keeps_the_original_deadline, async_compaction_rewind_preview_preserves_but_commit_invalidates_the_job, async_compaction_scenario, str (plus 32 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: async_compaction_promoted_job_with_incomplete_usage_cannot_commit
- **WHEN** 执行 async_compaction_promoted_job_with_incomplete_usage_cannot_commit 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: async_compaction_rejects_changed_model_route
- **WHEN** 执行 async_compaction_rejects_changed_model_route 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: async_compaction_failure_does_not_interrupt_or_restart_in_the_same_turn
- **WHEN** 执行 async_compaction_failure_does_not_interrupt_or_restart_in_the_same_turn 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/fs_injection_regression_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 tool_bridge_routes_writes_through_injected_fs。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/fs_injection_regression_tests.rs
- **THEN** 记录其 tool_bridge_routes_writes_through_injected_fs 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/fs_injection_regression_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/image_input_recovery_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 run_with_session_stack, actor_with_sampler, messages_text_turn, count_wire_images, explicit_image_400_without_description_fails_without_lossy_resubmission, active_goal_image_400_uses_auxiliary_description_then_retries_without_images, auxiliary_image_400_fails_without_installing_a_lossy_shadow。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_session_stack
- **WHEN** 执行 run_with_session_stack 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: actor_with_sampler
- **WHEN** 执行 actor_with_sampler 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: explicit_image_400_without_description_fails_without_lossy_resubmission
- **WHEN** 执行 explicit_image_400_without_description_fails_without_lossy_resubmission 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/image_input_recovery_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/interjection_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 end_active_test_turn, stale_explicit_steer_is_a_durable_blocked_human_intent, drain_interjections_pushes_synthetic_user_message_after_tool_result, main, TOOL_RESULT_CONTENT, drain_multiple_interjections_pushes_one_user_message_each_in_order, drain_with_empty_buffer_is_a_noop, terminal_boundary_discards_residual_same_turn_interjections, terminal_boundary_requeues_an_accepted_direct_steer, terminal_boundary_requeues_auto_promoted_follow_ups_and_discards_explicit, auto_promoted_entry_drained_at_safe_point_is_consumed_not_requeued, auto_promote_follow_up_uses_the_interjection_path_once, interjection_wraps_text_in_user_query, interjection_has_no_deferral_instruction, broadcast_interjection_emits_sessionid_and_text。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、session/timeline state projection、hook dispatch or hook source boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: end_active_test_turn
- **WHEN** 执行 end_active_test_turn 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: drain_with_empty_buffer_is_a_noop
- **WHEN** 执行 drain_with_empty_buffer_is_a_noop 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/interjection_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/laziness_debug_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 user_text, assistant_text, assistant_with_tool_call, assistant_with_reasoning_items, flatten_renders_roles_in_order_without_synthesising_an_assistant_turn, flatten_renders_tool_calls_as_lines, flatten_truncates_long_fields, flatten_collapses_newlines_to_keep_one_line_per_item, flatten_handles_system_items, flatten_renders_empty_input_as_empty_string, flatten_renders_assistant_reasoning, flatten_skips_reasoning_when_text_is_empty, flatten_skips_reasoning_when_text_is_whitespace_only, flatten_orders_reasoning_before_content_and_tools, flatten_truncates_long_reasoning_text, flatten_drops_reasoning_when_include_reasoning_is_false, flatten_keeps_reasoning_when_include_reasoning_is_true, synthetic_user_text (plus 19 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: assistant_with_tool_call
- **WHEN** 执行 assistant_with_tool_call 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: assistant_with_reasoning_items
- **WHEN** 执行 assistant_with_reasoning_items 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: flatten_renders_roles_in_order_without_synthesising_an_assistant_turn
- **WHEN** 执行 flatten_renders_roles_in_order_without_synthesising_an_assistant_turn 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/laziness_debug_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/laziness_detector_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 cfg_enabled, parse_classifier_output_clean_json, parse_classifier_output_stalled_false_completion_round_trips, parse_classifier_output_fence_wrapped, parse_classifier_output_fence_lowercase_and_uppercase_marker, parse_classifier_output_unfenced_with_trailing_prose, parse_classifier_output_brace_extract_handles_escaped_quotes, parse_classifier_output_truncated_json_returns_unparseable, parse_classifier_output_unknown_category_returns_unparseable, parse_classifier_output_confidence_above_one_is_rejected, parse_classifier_output_confidence_below_zero_is_rejected, parse_classifier_output_literal_nan_is_unparseable, parse_classifier_output_huge_finite_number_is_out_of_range, parse_classifier_output_brace_extract_handles_literal_braces_in_evidence, parse_classifier_output_bad_first_pass_does_not_short_circuit_when_other_passes_converge, parse_classifier_output_strict_unparseable_then_brace_extract_recovers, output, evaluate_laziness_observation_only_returns_nudge_cap_exhausted (plus 15 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: parse_classifier_output_unfenced_with_trailing_prose
- **WHEN** 执行 parse_classifier_output_unfenced_with_trailing_prose 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: parse_classifier_output_bad_first_pass_does_not_short_circuit_when_other_passes_converge
- **WHEN** 执行 parse_classifier_output_bad_first_pass_does_not_short_circuit_when_other_passes_converge 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: parse_classifier_output_strict_unparseable_then_brace_extract_recovers
- **WHEN** 执行 parse_classifier_output_strict_unparseable_then_brace_extract_recovers 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/laziness_detector_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/laziness_integration_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 detector_entry, make_laziness_actor, events_log, has_event_with, disabled_detector_is_a_no_op, user_input_bump_during_idle_wait_aborts_with_user_input, model_switch_during_idle_wait_aborts_with_model_switch, turn_start_ms_chain_feeds_turn_elapsed_seconds_helper, sampler_error_aborts_with_classifier_error, idle_recheck_after_sleep_short_circuits_silently, laziness_abort_check_detects_bumps_between_snapshot_and_recheck, model_switch_resets_nudges_used_this_session, emit_laziness_abort_writes_each_reason_with_the_correct_const, user_input_generation_bumped_only_on_real_prompts, arm_debug_log, make_debug_actor, debug_mode_fires_classifier_even_with_per_model_enable_false, debug_mode_bypasses_idle_wait (plus 1 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: user_input_bump_during_idle_wait_aborts_with_user_input
- **WHEN** 执行 user_input_bump_during_idle_wait_aborts_with_user_input 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: model_switch_during_idle_wait_aborts_with_model_switch
- **WHEN** 执行 model_switch_during_idle_wait_aborts_with_model_switch 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: sampler_error_aborts_with_classifier_error
- **WHEN** 执行 sampler_error_aborts_with_classifier_error 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/laziness_integration_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/mod.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 the file module entrypoint。源码显式使用 explicit error/result paths、child process lifecycle、session/timeline state projection、hook dispatch or hook source boundary、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/mod.rs
- **THEN** 记录其 the file module entrypoint 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/parallel_dispatch_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 test_parallel_dispatch_basic, str, test_parallel_dispatch_permission_reject, test_parallel_dispatch_followups, test_parallel_dispatch_hooks, incremental_dispatch_surfaces_fast_tool_before_slow_sibling, lock_path_for_args_matches_grow_build_file_path, lock_path_for_args_matches_path_arg, lock_path_for_args_matches_grow_build_target_file, lock_path_for_args_returns_none_for_pathless_tools, lock_path_for_args_ignores_non_string_path_values, lock_path_for_args_buckets_parallel_path_calls_to_same_lock, lock_path_for_args_normalizes_canonical_file_argument_names, test_skill_discovery_deferred_during_parallel_batch。源码显式使用 serde-backed wire/config types、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、hook dispatch or hook source boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_parallel_dispatch_basic
- **WHEN** 执行 test_parallel_dispatch_basic 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_parallel_dispatch_permission_reject
- **WHEN** 执行 test_parallel_dispatch_permission_reject 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_parallel_dispatch_followups
- **WHEN** 执行 test_parallel_dispatch_followups 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/parallel_dispatch_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/permission_auto_mode_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 dummy_gateway, acking_sideband_persistence, install_real_permissions, child_permission_judgment_branches_primary_context_without_mutation, PRIMARY_MARKER, CHILD_ID, live_child_judge_receives_primary_context_without_chat_state_pollution, chat_child_judge_retries_empty_invalid_and_transient_responses_once, set_auto_mode_path_wires_live_side_query_via_session_actor, spawn_auto_mode_wires_classifier_when_enabled, set_auto_mode_off_clears_side_query_flag, session_meta_permission_mode_resolution, neutralize_collapses_newline_and_defangs_forged_user_turn, neutralize_collapses_unicode_separators, neutralize_preserves_casing_when_defanging, neutralize_handles_multibyte_without_panic, permission_user, build_classifier_turns_captures_tool_use_excludes_text_and_results (plus 6 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: child_permission_judgment_branches_primary_context_without_mutation
- **WHEN** 执行 child_permission_judgment_branches_primary_context_without_mutation 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: live_child_judge_receives_primary_context_without_chat_state_pollution
- **WHEN** 执行 live_child_judge_receives_primary_context_without_chat_state_pollution 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: neutralize_preserves_casing_when_defanging
- **WHEN** 执行 neutralize_preserves_casing_when_defanging 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/permission_auto_mode_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/project_instructions_idempotence_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 detects_tagged_project_instructions_item, empty_conversation_returns_false, real_user_message_returns_false, site_a_skips_when_helper_returns_true_and_bumps_len_when_inserting, site_a_handles_none_inherited_prefix_len_without_panicking, site_a_skips_agents_md_insert_on_verbatim_mirror_fork, site_a_still_inserts_agents_md_on_non_fork_spawn, site_b_skips_agents_md_insert_on_verbatim_mirror_fork。源码显式使用 session/timeline state projection、git/worktree context、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: site_a_handles_none_inherited_prefix_len_without_panicking
- **WHEN** 执行 site_a_handles_none_inherited_prefix_len_without_panicking 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/project_instructions_idempotence_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/prompt_context_persistence_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 HEAD_TOKEN, TAIL_TOKEN, fake_prompt_path, fake_prompt_ref, str, truncate_bytes_suffix_is_utf8_safe, bound_head_tail_boundary_and_utf8, build_truncated_keeps_query_head_and_tail, build_truncated_preserves_small_query_truncates_context, build_truncated_both_oversized_keeps_bounded_heads, build_truncated_preserves_skill_information, build_truncated_bounds_oversized_skill_head_and_tail, build_truncated_multibyte_no_panic, build_offload_notice_reports_bytes_marker_and_ref, maybe_truncate_at_threshold_returns_unchanged_no_file, oversized_prompt_blob_is_owned_by_the_explicit_entity_directory, write_offload_and_build_wires_offload_and_fallback, prompt_blob_is_immutable_and_idempotent (plus 4 additional private symbols)。源码显式使用 filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: build_truncated_preserves_small_query_truncates_context
- **WHEN** 执行 build_truncated_preserves_small_query_truncates_context 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: build_truncated_preserves_skill_information
- **WHEN** 执行 build_truncated_preserves_skill_information 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: prompt_blob_write_rejects_symlinked_parent_directory
- **WHEN** 执行 prompt_blob_write_rejects_symlinked_parent_directory 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/prompt_context_persistence_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/prompt_mode_transition_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 behavior_gateway_rejects_agent_role_ids_instead_of_switching_to_normal, host_command_turn_can_apply_its_own_behavior_transition, fn_def, names, cursor_filter_in_plan_mode_keeps_writes_and_shows_create_plan, cursor_filter_is_noop_for_non_cursor_tools, plan_hides_workflow_launcher_but_default_keeps_it, prompt_mode_selects_exactly_one_behavior, reconcile。源码显式使用 serde-backed wire/config types、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: behavior_gateway_rejects_agent_role_ids_instead_of_switching_to_normal
- **WHEN** 执行 behavior_gateway_rejects_agent_role_ids_instead_of_switching_to_normal 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/prompt_mode_transition_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/read_file_image_description_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 image_tool_result, run_image_result, mark_current_model_as_text_only, test_actor, configured_auxiliary_does_not_preempt_unknown_current_model, known_text_only_model_degrades_read_file_image_before_sampling, compaction_refuses_to_erase_images_when_text_projection_is_unavailable, pdf_extracted_images_stay_one_ordered_group_and_only_the_text_route_is_projected, live_model_reload_updates_every_next_turn_sampler_knob, busy_model_reload_is_applied_before_the_next_idle_consumer。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_actor
- **WHEN** 执行 test_actor 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: configured_auxiliary_does_not_preempt_unknown_current_model
- **WHEN** 执行 configured_auxiliary_does_not_preempt_unknown_current_model 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/read_file_image_description_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/recap_display_only_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 sideband_persistence_harness, new_prompt_cancels_in_flight_recap_epoch, queue_input_user_prompt_bumps_recap_epoch, queue_input_synthetic_does_not_bump_recap_epoch, try_commit_recap_cancelled_clears_in_flight_without_watermark, try_commit_recap_live_advances_watermark, drop_recap_after_cancel_auto_silent_manual_unavailable, drained_session_recap, auto_recap_below_min_turns_is_noop_and_display_only, manual_recap_never_mutates_conversation, drained_recap_unavailable, wait_for_recap_unavailable, manual_recap_with_no_turns_emits_unavailable, manual_recap_generation_failure_emits_unavailable, manual_recap_generation_failure_records_sideband, auto_recap_gated_does_not_emit_unavailable, manual_recap_over_budget_is_display_only_and_references_timeline, over_budget_recap_serializes_to_well_formed_messages_request (plus 2 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: queue_input_synthetic_does_not_bump_recap_epoch
- **WHEN** 执行 queue_input_synthetic_does_not_bump_recap_epoch 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: try_commit_recap_cancelled_clears_in_flight_without_watermark
- **WHEN** 执行 try_commit_recap_cancelled_clears_in_flight_without_watermark 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: manual_recap_with_no_turns_emits_unavailable
- **WHEN** 执行 manual_recap_with_no_turns_emits_unavailable 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/recap_display_only_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/record_response_token_usage_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 response_with_usage, response_without_usage, quarantined_response_is_billed_without_restoring_its_context_anchor, anchors_projected_context_from_response_usage, goal_usage_accumulates_model_consumption_when_context_pressure_falls, descendant_model_usage_is_submitted_to_the_root_goal_window, preserves_projection_when_response_has_no_usage, final_request_schema_is_visible_to_pre_sampling_pressure, build_session_info_used_reflects_recorded_response, build_session_info_sources_show_model_fingerprint_from_catalog, stashes_per_turn_usage_in_chat_state。源码显式使用 serde-backed wire/config types、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: response_with_usage
- **WHEN** 执行 response_with_usage 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: response_without_usage
- **WHEN** 执行 response_without_usage 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: quarantined_response_is_billed_without_restoring_its_context_anchor
- **WHEN** 执行 quarantined_response_is_billed_without_restoring_its_context_anchor 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/record_response_token_usage_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/reminder_policy_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 ymd, date_rollover_reminder_silent_when_same_day, date_rollover_reminder_fires_when_day_advances, date_rollover_reminder_fires_across_month_and_year_boundaries, date_rollover_reminder_silent_when_clock_moves_backward, same_session_rolls_over_once_when_local_date_advances。源码显式使用 channel or acknowledgement flow、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/reminder_policy_tests.rs
- **THEN** 记录其 ymd, date_rollover_reminder_silent_when_same_day, date_rollover_reminder_fires_when_day_advances, date_rollover_reminder_fires_across_month_and_year_boundaries, date_rollover_reminder_silent_when_clock_moves_backward, same_session_rolls_over_once_when_local_date_advances 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/reminder_policy_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/reverse_request_session_id_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 ask_user_question_request_carries_session_id, plan_approval_request_carries_session_id。源码显式使用 serde-backed wire/config types、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/reverse_request_session_id_tests.rs
- **THEN** 记录其 ask_user_question_request_carries_session_id, plan_approval_request_carries_session_id 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/reverse_request_session_id_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/rewind_synthetic_turn_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 seed_conversation, run_rewind_over_synthetic_turn, rewind_removes_turn_after_synthetic_auto_wake_unmarked, rewind_removes_turn_after_synthetic_auto_wake_marked, rewind_with_no_prompts_lists_no_points_and_rejects_execute, rewind_to_start_keeps_only_preamble, rewind_twice_narrows_history_each_time, rewind_to_midpoint_with_synthetic_turns_on_both_sides, rewind_to_synthetic_auto_wake_turn_cuts_at_the_wake。源码显式使用 explicit error/result paths、channel or acknowledgement flow、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: rewind_with_no_prompts_lists_no_points_and_rejects_execute
- **WHEN** 执行 rewind_with_no_prompts_lists_no_points_and_rejects_execute 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: rewind_to_midpoint_with_synthetic_turns_on_both_sides
- **WHEN** 执行 rewind_to_midpoint_with_synthetic_turns_on_both_sides 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/rewind_synthetic_turn_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/session_thread_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 wait_for_finish, FINISH_TIMEOUT, session_thread_detects_normal_exit, session_thread_detects_panic, session_thread_not_finished_while_running, sessions_on_separate_threads_do_not_block_each_other。源码显式使用 channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/src/session/actor/tests/session_thread_tests.rs
- **THEN** 记录其 wait_for_finish, FINISH_TIMEOUT, session_thread_detects_normal_exit, session_thread_detects_panic, session_thread_not_finished_while_running, sessions_on_separate_threads_do_not_block_each_other 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/src/session/actor/tests/session_thread_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/stop_cancelled_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 END_TURN, run_with_session_stack, text_block, messages_turn, actor_with_sampler, run_user_turn, ObservedHook, drain_hook_observations, stop_cancelled_emitted_with_reason_and_never_blocks_cancel, stop_cancelled_not_emitted_without_a_cancelled_turn。源码显式使用 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_session_stack
- **WHEN** 执行 run_with_session_stack 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: actor_with_sampler
- **WHEN** 执行 actor_with_sampler 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: stop_cancelled_emitted_with_reason_and_never_blocks_cancel
- **WHEN** 执行 stop_cancelled_emitted_with_reason_and_never_blocks_cancel 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/stop_cancelled_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/subagent_bash_permission_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 SUBAGENT_SID, PARENT_SID, BASH_ARGS, TOOL_CALL_ID, agent_switch_reprojects_native_identity_without_widening_child_rwx, agent_switch_reprojects_mcp_inheritance_without_exceeding_spawn_ceiling, ImmediateSubagentBackend, spawn, query, cancel, validate_type, GatewayLog, spawn_gateway_responder, str, is, make_subagent_fixture, make_subagent_fixture_with_replies, drain_notifications_to_gateway (plus 16 additional private symbols)。源码显式使用 filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: agent_switch_reprojects_native_identity_without_widening_child_rwx
- **WHEN** 执行 agent_switch_reprojects_native_identity_without_widening_child_rwx 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: agent_switch_reprojects_mcp_inheritance_without_exceeding_spawn_ceiling
- **WHEN** 执行 agent_switch_reprojects_mcp_inheritance_without_exceeding_spawn_ceiling 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: make_subagent_fixture_with_replies
- **WHEN** 执行 make_subagent_fixture_with_replies 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/subagent_bash_permission_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/support.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 admit_test_human_input, test_auth_method_id, begin_test_causal_turn, begin_test_causal_turn_with_identity, begin_test_active_causal_turn, begin_test_active_causal_turn_with_origin, replace_test_surface, seed_test_timeline, record_test_prompt, NEXT_TURN, test_agent_default, test_grow_build_agent_with_todo, test_agent_with_plan_tools, test_agent_with_tools, test_agent_from_config, DummyTerminal, run, create_test_actor_ex (plus 8 additional private symbols)。源码显式使用 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: begin_test_causal_turn_with_identity
- **WHEN** 执行 begin_test_causal_turn_with_identity 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_agent_from_config
- **WHEN** 执行 test_agent_from_config 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/support.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/truncation_recovery_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 prompt, MAX_TOKENS, END_TURN, CONTEXT_WINDOW_EXCEEDED, PAUSE_TURN, text_block, thinking_block, tool_use_block, tool_use_block_incomplete, messages_turn, chat_completions_turn, run_with_session_stack, actor_with_sampler, run_user_turn, seed_closed_compaction_range, assistant_texts, truncation_continue_count, drain_hook_event_names (plus 19 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_session_stack
- **WHEN** 执行 run_with_session_stack 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: actor_with_sampler
- **WHEN** 执行 actor_with_sampler 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: failed_portable_retry_notifies_once_without_poisoning_session
- **WHEN** 执行 failed_portable_retry_notifies_once_without_poisoning_session 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/truncation_recovery_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/turn_pipeline_v2_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 run_with_session_stack, foreground_snapshot_carries_origin_and_kind_without_parsing_its_id, completed_runner_keeps_foreground_fenced_until_terminal_settlement, goal_failure_keeps_its_origin_across_terminalization, active_control_runtimes_keep_an_idle_session_resident, only_real_user_input_can_supply_an_implicit_goal_objective, implicit_goal_objective_commits_its_turn_terminal_before_continuation, autonomous_first_turn_commits_the_deferred_prefix_before_turn_started, paused_unbudgeted_goal_with_incomplete_usage_can_restart_explicitly, unexpected_turn_owner_failure_closes_every_open_causal_child, explicit_cancel_closes_request_tool_step_then_turn, panic_after_durable_turn_terminal_never_appends_a_second_terminal, every_continuation_audits_the_full_goal_before_planning_a_local_slice, goal_runtime_requires_the_local_task_planner。源码显式使用 explicit error/result paths、channel or acknowledgement flow、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_session_stack
- **WHEN** 执行 run_with_session_stack 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: foreground_snapshot_carries_origin_and_kind_without_parsing_its_id
- **WHEN** 执行 foreground_snapshot_carries_origin_and_kind_without_parsing_its_id 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: paused_unbudgeted_goal_with_incomplete_usage_can_restart_explicitly
- **WHEN** 执行 paused_unbudgeted_goal_with_incomplete_usage_can_restart_explicitly 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/turn_pipeline_v2_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/usage_categories_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 mcp_tool, install_mcp_servers, seed_skills, usage_categories_include_skills_and_mcp_with_counts, mcp_snapshot_matches_full_mode_injected_reminder。源码显式使用 serde-backed wire/config types、channel or acknowledgement flow、session/timeline state projection、MCP integration boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: usage_categories_include_skills_and_mcp_with_counts
- **WHEN** 执行 usage_categories_include_skills_and_mcp_with_counts 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/usage_categories_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tests/workflow_launch_tests.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 workflow_behavior_and_commands_follow_runtime_not_agent_tools, saved_workflow_dynamic_command_with_agent_preflight_creates_run, handwritten_workflow_launch_cannot_bypass_disabled_feature_gate。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、async task lifecycle and cancellation、channel or acknowledgement flow、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: saved_workflow_dynamic_command_with_agent_preflight_creates_run
- **WHEN** 执行 saved_workflow_dynamic_command_with_agent_preflight_creates_run 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/src/session/actor/tests/workflow_launch_tests.rs`。

### Requirement: Shell crates/codegen/shell/tests/common/mod.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 TestBody, PanicPayload, finish_body, CleanupClient, graceful_close, hard_close, contain_failed_cleanup_for_unwind, CleanupFixture, close_fixture, ClientCleanupOutcome, close_clients, cleanup_owned_processes, run_with_cleanup, FakeClient, drop, FakeFixture, double_failed_client_transfers_to_unwind_containment, successful_owners_drop_before_fixture_close (plus 3 additional private symbols)。源码显式使用 explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_cleanup
- **WHEN** 执行 run_with_cleanup 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: create_test_client
- **WHEN** 执行 create_test_client 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: create_test_client_with_extra_headers
- **WHEN** 执行 create_test_client_with_extra_headers 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/common/mod.rs`。

### Requirement: Shell crates/codegen/shell/tests/git_contention_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 DUPLEX_BUFFER_BYTES, git, ScanCounter, scans, skips, install_global_scan_counter, build_repo, chat_chunk, tool_call_sse, CALL_SEQ, text_sse, AutoApproveClient, request_permission, session_notification, RunStats, prompt_turn, run_storm, git_rebase_refresh_storm_e2e。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/git_contention_e2e.rs
- **THEN** 记录其 DUPLEX_BUFFER_BYTES, git, ScanCounter, scans, skips, install_global_scan_counter, build_repo, chat_chunk, tool_call_sse, CALL_SEQ, text_sse, AutoApproveClient, request_permission, session_notification, RunStats, prompt_turn, run_storm, git_rebase_refresh_storm_e2e 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/git_contention_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/memory_integration/run_tests.py test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 the file module entrypoint。源码显式使用 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/memory_integration/run_tests.py
- **THEN** 记录其 the file module entrypoint 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/memory_integration/run_tests.py`。

### Requirement: Shell crates/codegen/shell/tests/session_fork_replay_memory.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 CountingAlloc, LIVE, PEAK, alloc, dealloc, ALLOC, begin_measure, peak, fork_replay_spec, reference_load_all, serialize, MAX_STREAM_PEAK_TO_DISK_RATIO, fork_replay_stream_is_bounded_and_faithful。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、platform or feature-gated branches、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/session_fork_replay_memory.rs
- **THEN** 记录其 CountingAlloc, LIVE, PEAK, alloc, dealloc, ALLOC, begin_measure, peak, fork_replay_spec, reference_load_all, serialize, MAX_STREAM_PEAK_TO_DISK_RATIO, fork_replay_stream_is_bounded_and_faithful 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/session_fork_replay_memory.rs`。

### Requirement: Shell crates/codegen/shell/tests/session_load_perf.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 perf_spec, prepare_session, generate_or_restore_rewind, KindStats, file_size_mb, file_fingerprint, updates_kind_breakdown, print_kind_breakdown, phase_breakdown_real_functions, LoadCounters, CountingClient, request_permission, session_notification, parse_instrumentation_log, full_session_load_e2e。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/session_load_perf.rs
- **THEN** 记录其 perf_spec, prepare_session, generate_or_restore_rewind, KindStats, file_size_mb, file_fingerprint, updates_kind_breakdown, print_kind_breakdown, phase_breakdown_real_functions, LoadCounters, CountingClient, request_permission, session_notification, parse_instrumentation_log, full_session_load_e2e 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/session_load_perf.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_active_sessions_smoke.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 session, full_lifecycle。源码显式使用 filesystem or durable record I/O、child process lifecycle、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_active_sessions_smoke.rs
- **THEN** 记录其 session, full_lifecycle 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_active_sessions_smoke.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_auth_provider_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 provider_backed_model_sends_minted_token_on_the_wire, undefined_provider_fails_closed_and_never_leaks_session_key, provider_with_args_and_json_output_sends_minted_token。源码显式使用 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: undefined_provider_fails_closed_and_never_leaks_session_key
- **WHEN** 执行 undefined_provider_fails_closed_and_never_leaks_session_key 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: provider_with_args_and_json_output_sends_minted_token
- **WHEN** 执行 provider_with_args_and_json_output_sends_minted_token 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_auth_provider_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_built_binary_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 with_local_set, CHAT_COMPLETIONS_MODEL, single_model_server, grow_build_server, parse_stdout_json, request_tool_name, inference_request, inference_tool_names, test_version_exits_zero, layouts, test_version_with_crash_handler_exits_zero, test_headless_session_in_git_repo, test_headless_session_in_non_git_dir, test_headless_tools_allowlist_keeps_enabled_web_fetch, test_headless_tools_allowlist_does_not_fail_open_for_disabled_web_fetch, test_headless_terminal_only_allowlist_is_foreground_only, test_headless_streaming_json_output, test_headless_streaming_messages_json_output (plus 27 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: with_local_set
- **WHEN** 执行 with_local_set 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_version_exits_zero
- **WHEN** 执行 test_version_exits_zero 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_version_with_crash_handler_exits_zero
- **WHEN** 执行 test_version_with_crash_handler_exits_zero 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_built_binary_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_debug_logging.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 with_local_set, debug_dir, firehose_txt_files, debug_cmd, read_session_firehose_when_ready, debug_flag_enables_firehose_without_crashing, no_debug_flag_writes_no_debug_dir, agent_session_writes_named_session_file, debug_flag_master_switch_enables_firehose, debug_file_flag_writes_single_file_and_bypasses_routing, grow_log_file_explicit_path_is_written。源码显式使用 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: with_local_set
- **WHEN** 执行 with_local_set 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: debug_flag_enables_firehose_without_crashing
- **WHEN** 执行 debug_flag_enables_firehose_without_crashing 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_debug_logging.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_doom_loop_recovery.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 install_rustls_provider, MODEL, doom_loop_client, spawn_actor, user_request, responses_request_count, mid_stream_check_frames_populate_and_dedupe_signals, byte_exact_cumulative_frame_parses_through_the_client, terminal_only_field_populates_signals, malformed_check_frames_complete_cleanly_without_signals, unknown_extra_keys_still_parse, absent_field_leaves_signals_empty, unknown_label_kinds_preserved_as_unknown, disabled_policy_leaves_terminal_field_unparsed, confident_signal_resamples_once_and_discards_poisoned_turn, budget_exhaustion_accepts_last_doomed_response, not_confident_signals_do_not_resample, disabled_policy_ignores_confident_signal (plus 3 additional private symbols)。源码显式使用 filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: byte_exact_cumulative_frame_parses_through_the_client
- **WHEN** 执行 byte_exact_cumulative_frame_parses_through_the_client 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: malformed_check_frames_complete_cleanly_without_signals
- **WHEN** 执行 malformed_check_frames_complete_cleanly_without_signals 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: budget_exhaustion_accepts_last_doomed_response
- **WHEN** 执行 budget_exhaustion_accepts_last_doomed_response 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_doom_loop_recovery.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_doomloop_capture.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 install_rustls_provider, responses_api_reasoning_only_is_classified_as_reasoning_only, normal_text_response_is_not_classified_reasoning_only。源码显式使用 session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_doomloop_capture.rs
- **THEN** 记录其 install_rustls_provider, responses_api_reasoning_only_is_classified_as_reasoning_only, normal_text_response_is_not_classified_reasoning_only 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_doomloop_capture.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_fork_session.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 create_test_session, test_fork_session_creates_new_session_with_parent_tracking, test_fork_starts_new_title_lineage。源码显式使用 filesystem or durable record I/O、session/timeline state projection、git/worktree context、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: create_test_session
- **WHEN** 执行 create_test_session 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_fork_session_creates_new_session_with_parent_tracking
- **WHEN** 执行 test_fork_session_creates_new_session_with_parent_tracking 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_fork_starts_new_title_lineage
- **WHEN** 执行 test_fork_starts_new_title_lineage 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_fork_session.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_global_extra_headers_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 global_models_config_reaches_inference_request。源码显式使用 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_global_extra_headers_e2e.rs
- **THEN** 记录其 global_models_config_reaches_inference_request 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_global_extra_headers_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_grow_session_update.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 test_grow_session_notification_storage_roundtrip, new, old, test_turn_completed_round_trips_through_storage, test_extract_total_tokens_from_mixed_updates, test_grow_session_notification_serialization。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_grow_session_notification_storage_roundtrip
- **WHEN** 执行 test_grow_session_notification_storage_roundtrip 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_turn_completed_round_trips_through_storage
- **WHEN** 执行 test_turn_completed_round_trips_through_storage 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_extract_total_tokens_from_mixed_updates
- **WHEN** 执行 test_extract_total_tokens_from_mixed_updates 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_grow_session_update.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_leader_death_repro.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 test_leader_sigkill_clients_recover_sessions, test_leader_sigkill_single_client_recovers, test_leader_sigkill_multi_session_client_recovers_all_sessions, test_prompt_sent_during_outage_is_delivered_after_recovery。源码显式使用 platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、sandbox/trust boundary、git/worktree context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_leader_sigkill_clients_recover_sessions
- **WHEN** 执行 test_leader_sigkill_clients_recover_sessions 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_leader_sigkill_single_client_recovers
- **WHEN** 执行 test_leader_sigkill_single_client_recovers 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_leader_sigkill_multi_session_client_recovers_all_sessions
- **WHEN** 执行 test_leader_sigkill_multi_session_client_recovers_all_sessions 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_leader_death_repro.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_leader_sandbox_confinement.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 connect_or_spawn_refuses_when_sandbox_confinement_requested。源码显式使用 explicit error/result paths、session/timeline state projection、sandbox/trust boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_leader_sandbox_confinement.rs
- **THEN** 记录其 connect_or_spawn_refuses_when_sandbox_confinement_requested 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_leader_sandbox_confinement.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_leader_soak.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 DHAT_ALLOC, HEAP_WARMUP_CYCLES, env_u64, send_failed_count, rpc, registry_counts, leader_soak_churning_clients_no_leaks_no_zombies。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_leader_soak.rs
- **THEN** 记录其 DHAT_ALLOC, HEAP_WARMUP_CYCLES, env_u64, send_failed_count, rpc, registry_counts, leader_soak_churning_clients_no_leaks_no_zombies 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_leader_soak.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_leader_stdio_integration.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 ID_NAMESPACE_SEP, wait_for_socket, setup_test_server, setup_control_test_server, parse_namespaced_id, namespaced_id_has_original, test_single_stdio_client_connects, test_stdio_client_sends_acp_message, test_stdio_client_receives_response, test_multiple_stdio_clients, test_multiple_clients_same_message_ids, test_stdio_client_disconnect, test_stdio_client_ping_pong, test_server_exits_when_all_clients_disconnect, test_runtime_profile_start_status_stop_across_clients, test_runtime_profile_finalizes_on_graceful_shutdown, test_runtime_profile_creates_missing_parent_directory_end_to_end, test_runtime_profile_rejects_output_collision_end_to_end (plus 43 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: setup_test_server
- **WHEN** 执行 setup_test_server 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: setup_control_test_server
- **WHEN** 执行 setup_control_test_server 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_single_stdio_client_connects
- **WHEN** 执行 test_single_stdio_client_connects 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_leader_stdio_integration.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_mcp_integration.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 echo_tool, test_schema_json_conversion, test_echo_tool_schema_has_required_field, ui_tool, str, test_meta_ui_survives_serialization_roundtrip, test_visibility_app_only_hides_from_model, test_mcp_tool_schema_preserves_required, test_mcp_registration_carries_server_schema_not_schemars_derived, test_empty_mcp_schema_gets_type_object_injected。源码显式使用 serde-backed wire/config types、platform or feature-gated branches、MCP integration boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_schema_json_conversion
- **WHEN** 执行 test_schema_json_conversion 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_echo_tool_schema_has_required_field
- **WHEN** 执行 test_echo_tool_schema_has_required_field 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_meta_ui_survives_serialization_roundtrip
- **WHEN** 执行 test_meta_ui_survives_serialization_roundtrip 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_mcp_integration.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_mcp_permission_persistence.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 test_home, PathBuf, HOME, fresh_cwd, permission_state_path, load_state, tool_call_update, make_session_id, FakeGateway, fake_gateway, expect_allow_always_mcp_tool, expect_allow_always_mcp_server, expect_allow_always_mcp_without_scope, request, mcp, run_actor_test, run_actor_test_with_policy, run_actor_test_full (plus 18 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_home
- **WHEN** 执行 test_home 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: expect_allow_always_mcp_without_scope
- **WHEN** 执行 expect_allow_always_mcp_without_scope 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: run_actor_test
- **WHEN** 执行 run_actor_test 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_mcp_permission_persistence.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_nonblocking_startup.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 poll_until, leader_ready_while_proxy_hangs, catalog_self_heals_after_endpoint_recovers, leader_usable_when_settings_blocked_but_models_served。源码显式使用 platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、sandbox/trust boundary、git/worktree context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: catalog_self_heals_after_endpoint_recovers
- **WHEN** 执行 catalog_self_heals_after_endpoint_recovers 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_nonblocking_startup.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_nonblocking_startup_offline.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 closed_port_base_url, assert_boots_fast, str, leader_ready_with_connection_refused。源码显式使用 explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、sandbox/trust boundary；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: leader_ready_with_connection_refused
- **WHEN** 执行 leader_ready_with_connection_refused 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_nonblocking_startup_offline.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_refusal_stop_reason.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 with_local_set, refusal_messages_server, turn_messages_request_count, test_refusal_turn_completes_with_single_messages_request, test_leader_refusal_turn_completes_with_single_messages_request。源码显式使用 explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: with_local_set
- **WHEN** 执行 with_local_set 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_refusal_turn_completes_with_single_messages_request
- **WHEN** 执行 test_refusal_turn_completes_with_single_messages_request 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_leader_refusal_turn_completes_with_single_messages_request
- **WHEN** 执行 test_leader_refusal_turn_completes_with_single_messages_request 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_refusal_stop_reason.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_registry_churn.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 DUPLEX_BUFFER_BYTES, CHURN_SESSIONS, CONCURRENT_SESSIONS, RPC_TIMEOUT, test_model_config, Counts, AutoApproveClient, request_permission, session_notification, ext_method, read_counts, new_session, prompt_turn, close_session, churn_one, connect_and_auth, session_churn_returns_registry_snapshot_to_baseline。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: test_model_config
- **WHEN** 执行 test_model_config 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_registry_churn.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_sampling_client.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 install_rustls_provider, chat_completion_tool_call_stream, chat_completion_with_reasoning_stream, responses_api_tool_call_stream, test_chat_completions_streaming_text, test_chat_completions_streaming_tool_calls, test_chat_completions_with_reasoning, chat_completions_collect_synthesizes_reasoning_sibling, responses_api_with_reasoning_stream, test_responses_api_streaming_text, test_responses_api_streaming_tool_call, test_responses_api_with_reasoning_and_encrypted_content, test_responses_api_reasoning_without_encrypted, test_chat_completions_401_unauthorized, test_chat_completions_500_server_error, test_responses_api_401_unauthorized, test_stream_error_during_streaming, test_stream_error_during_responses_streaming (plus 12 additional private symbols)。源码显式使用 serde-backed wire/config types、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: chat_completion_with_reasoning_stream
- **WHEN** 执行 chat_completion_with_reasoning_stream 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_chat_completions_streaming_text
- **WHEN** 执行 test_chat_completions_streaming_text 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_chat_completions_streaming_tool_calls
- **WHEN** 执行 test_chat_completions_streaming_tool_calls 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_sampling_client.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_session_end_hook_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 run_with_session_end_hook, session_end_hook_fires_on_headless_exit。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_session_end_hook
- **WHEN** 执行 run_with_session_end_hook 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_session_end_hook_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_session_load_memory.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 DHAT_ALLOC, BYTES_PER_MB, BYTES_PER_MB_U64, file_len, memory_spec, prepare_replay_lines_borrows_the_transcript, quiesce, YIELDS, SETTLE, run_load_cycle, DhatBudget, DhatMeasured, DhatOutcome, ratio_budget_bytes, abs_budget_bytes, per_cycle_bytes, per_cycle_blocks, no_spike (plus 24 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_session_load_memory.rs
- **THEN** 记录其 DHAT_ALLOC, BYTES_PER_MB, BYTES_PER_MB_U64, file_len, memory_spec, prepare_replay_lines_borrows_the_transcript, quiesce, YIELDS, SETTLE, run_load_cycle, DhatBudget, DhatMeasured, DhatOutcome, ratio_budget_bytes, abs_budget_bytes, per_cycle_bytes, per_cycle_blocks, no_spike (plus 24 additional private symbols) 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_session_load_memory.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_session_protocol_recovery.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 BACKENDS, terminal_model_failure_count, tools_reply, foreground_bodies, terminal_tool_name, has_retained_result, assert_clean_history, protocol_failure_preserves_session_across_continue_switch_and_process_restart。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: protocol_failure_preserves_session_across_continue_switch_and_process_restart
- **WHEN** 执行 protocol_failure_preserves_session_across_continue_switch_and_process_restart 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_session_protocol_recovery.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_stop_hook_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 StopHookRun, invocations, hook_input, some_request_contains, run_with_stop_hook, assert_success, stop_block_keeps_agent_working_then_allows, stop_exit_2_blocks_with_stderr_feedback, stop_continue_false_overrides_block, stop_block_loop_ends_at_continuation_cap。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: run_with_stop_hook
- **WHEN** 执行 run_with_stop_hook 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: stop_exit_2_blocks_with_stderr_feedback
- **WHEN** 执行 stop_exit_2_blocks_with_stderr_feedback 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_stop_hook_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_subagent_orphan_reconcile.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 with_local_set, locate_session_dir, resume_reconciles_orphaned_running_subagent。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: with_local_set
- **WHEN** 执行 with_local_set 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_subagent_orphan_reconcile.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_summary_reasoning_effort.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 with_local_set, read_summary, test_fresh_session_persists_reasoning_effort, test_fresh_session_without_effort_omits_field。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: with_local_set
- **WHEN** 执行 with_local_set 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_fresh_session_persists_reasoning_effort
- **WHEN** 执行 test_fresh_session_persists_reasoning_effort 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: test_fresh_session_without_effort_omits_field
- **WHEN** 执行 test_fresh_session_without_effort_omits_field 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/test_summary_reasoning_effort.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_tool_dispatch_duration_smoke.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 CALL_ID, SLEEP_SECS, enqueue_sleep_tool_turn, find_tool_completed, collect_timeline_jsonl, walk, sleep_tool_records_multi_second_dispatch_duration。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_tool_dispatch_duration_smoke.rs
- **THEN** 记录其 CALL_ID, SLEEP_SECS, enqueue_sleep_tool_turn, find_tool_completed, collect_timeline_jsonl, walk, sleep_tool_records_multi_second_dispatch_duration 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_tool_dispatch_duration_smoke.rs`。

### Requirement: Shell crates/codegen/shell/tests/test_trusted_local_plugin_refresh_e2e.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 write_minimal_plugin, write_agent, register_local_install, EnvVarGuard, str, set, drop, trusted_local_refresh_surfaces_new_agent_via_discovery, headless_session_refreshes_trusted_local_plugin_and_writes_session_json, collect_json_files。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/tests/test_trusted_local_plugin_refresh_e2e.rs
- **THEN** 记录其 write_minimal_plugin, write_agent, register_local_install, EnvVarGuard, str, set, drop, trusted_local_refresh_surfaces_new_agent_via_discovery, headless_session_refreshes_trusted_local_plugin_and_writes_session_json, collect_json_files 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/tests/test_trusted_local_plugin_refresh_e2e.rs`。

### Requirement: Shell crates/codegen/shell/tests/testkit_synth_roundtrip.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 synth_replay_roundtrip_parses_every_persisted_update, synthesize_to_target_bytes_reaches_target_and_parses。源码显式使用 filesystem or durable record I/O、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: synth_replay_roundtrip_parses_every_persisted_update
- **WHEN** 执行 synth_replay_roundtrip_parses_every_persisted_update 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

#### Scenario: synthesize_to_target_bytes_reaches_target_and_parses
- **WHEN** 执行 synthesize_to_target_bytes_reaches_target_and_parses 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/tests/testkit_synth_roundtrip.rs`。

### Requirement: Shell crates/codegen/shell/benches/fork_copy.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 bench_fork_copy。源码显式使用 filesystem or durable record I/O、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/benches/fork_copy.rs
- **THEN** 记录其 bench_fork_copy 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/benches/fork_copy.rs`。

### Requirement: Shell crates/codegen/shell/benches/session_list.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 WORKSPACE_COUNT, FIXTURE_SCHEMA_VERSION, MAIN_CHECKOUT_COUNT, LINKED_WORKTREE_COUNT, DB_OVERLAP_WORKTREE_COUNT, DB_ONLY_WORKTREE_COUNT, FILESYSTEM_ONLY_WORKTREE_COUNT, DB_TRACKED_WORKTREE_COUNT, SAME_REPO_CANDIDATE_COUNT, SESSIONS_PER_SAME_REPO_CWD, SESSIONS_PER_UNRELATED_WORKSPACE, SAME_REPO_SUMMARY_COUNT, TOTAL_SUMMARY_COUNT, RECENT_LIMIT, SAMPLE_SIZE, ACTIVITY_ROTATION, COOPERATIVE_PEER_DELAY, CandidateSource (plus 14 additional private symbols)。源码显式使用 serde-backed wire/config types、filesystem or durable record I/O、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: build_local_list_with_delayed_peer
- **WHEN** 执行 build_local_list_with_delayed_peer 所描述的测试场景
- **THEN** 该场景检验 test and benchmark harness 的对应边界；通过状态须由后续测试运行记录。

证据：`crates/codegen/shell/benches/session_list.rs`。

### Requirement: Shell crates/codegen/shell/benches/skills_watcher_startup.rs test and benchmark harness contract

该测试/基准文件 SHALL 作为 test and benchmark harness 的静态证据，覆盖入口 DEFAULT_WORKTREE_DIRS, worktree_dir_count, make_nested_dirs, Fixture, build_fixture, start_scoped, start_recursive_control, bench_skills_watcher_startup。源码显式使用 filesystem or durable record I/O、timeout/deadline or timing decisions、git/worktree context；本条只记录断言边界和依赖条件，不把源码阅读当成运行通过。

#### Scenario: Harness inventory
- **WHEN** 读取 crates/codegen/shell/benches/skills_watcher_startup.rs
- **THEN** 记录其 DEFAULT_WORKTREE_DIRS, worktree_dir_count, make_nested_dirs, Fixture, build_fixture, start_scoped, start_recursive_control, bench_skills_watcher_startup 入口及外部依赖，未推断执行结果。

证据：`crates/codegen/shell/benches/skills_watcher_startup.rs`。
### Requirement: Tools crates/codegen/tools/Cargo.toml crate manifest and feature boundary contract
Cargo.toml SHALL retain the tools package feature boundary: default and default-bazel enable serde, serde is explicit, and dhat-heap enables optional dhat; platform-specific unix/linux/windows dependencies and the build-time reqwest/flate2/sha2/tar boundary remain manifest-declared. No dependency resolution is inferred.

#### Scenario: Feature selection
- **WHEN** the crate is built with default features
- **THEN** serde is enabled by the manifest and dhat remains opt-in.

#### Scenario: Platform dependency
- **WHEN** a target-specific dependency section is selected
- **THEN** only the declared target section contributes dependencies; this audit does not compile it.

证据：`crates/codegen/tools/Cargo.toml` — `[features]`；`crates/codegen/tools/Cargo.toml` — `default = ["serde"]`；`crates/codegen/tools/Cargo.toml` — `default-bazel = ["serde"]`；`crates/codegen/tools/Cargo.toml` — `dhat-heap = ["dep:dhat"]`。

### Requirement: Tools crates/codegen/tools/THIRD_PARTY_NOTICES.md third-party notice boundary contract
THIRD_PARTY_NOTICES.md SHALL record the bundled helper binaries and their required license/notice text; it is an audit resource for build redistribution and does not prove that a binary was downloaded or executed.

#### Scenario: Notice lookup
- **WHEN** a bundled binary is distributed
- **THEN** its corresponding third-party notice remains available in this file.

证据：`crates/codegen/tools/THIRD_PARTY_NOTICES.md` — `and`；`crates/codegen/tools/THIRD_PARTY_NOTICES.md` — `binaries`；`crates/codegen/tools/THIRD_PARTY_NOTICES.md` — `ugrep`。

### Requirement: Tools crates/codegen/tools/schema/tool_meta.schema.json tool metadata schema contract
tool_meta.schema.json SHALL define the accepted tool metadata object shape, including name/description/parameters and the schema constraints consumed by schemars/serde metadata validation; this resource does not execute a tool.

#### Scenario: Metadata schema
- **WHEN** a tool metadata document is validated
- **THEN** only fields and value shapes declared by the JSON schema are accepted.

证据：`crates/codegen/tools/schema/tool_meta.schema.json` — `"name"`；`crates/codegen/tools/schema/tool_meta.schema.json` — `"properties"`；`crates/codegen/tools/schema/tool_meta.schema.json` — `"namespace"`；`crates/codegen/tools/schema/tool_meta.schema.json` — `"input"`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs integration test harness contract
crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs SHALL provide static integration evidence for integration test harness; named entrypoints include MOCK_LSP_SERVER, write_mock_server, write_delayed_diagnostics_server, DELAYED_SERVER, write_init_failure_server, write_slow_init_server, write_init_failure_server_n_times, MOCK_PREAMBLE, write_python_server, write_incremental_sync_server, write_pull_diagnostics_server, write_selective_pull_server, write_mid_analysis_pull_server, write_slow_pull_server, FIRST_PULL_MARKER, SECOND_PULL_MARKER, write_stale_clean_pull_server, write_partially_answering_server. The implementation exercises manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work and explicit markers filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; source review does not claim execution success.

#### Scenario: Harness scope
- **WHEN** crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs is loaded by the test/build harness
- **THEN** its external prerequisites and static entrypoints are recorded without runtime claims.

证据：`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `MOCK_LSP_SERVER`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_mock_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_delayed_diagnostics_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `DELAYED_SERVER`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_init_failure_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_slow_init_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_init_failure_server_n_times`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `MOCK_PREAMBLE`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_python_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_incremental_sync_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_pull_diagnostics_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_selective_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_mid_analysis_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_slow_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `FIRST_PULL_MARKER`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `SECOND_PULL_MARKER`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_stale_clean_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_partially_answering_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_silent_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_save_with_text_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_loads_late_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_diagnostic_refresh_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_versioned_push_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_push_and_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_publishes_before_open_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_refresh_without_pull_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_pull_rejecting_server`；`crates/codegen/tools/src/implementations/lsp/tests/mock_servers.rs` — `write_clean_then_silent_server`。

### Requirement: Tools crates/codegen/tools/tests/agents_md_discovery.rs integration test harness contract
crates/codegen/tools/tests/agents_md_discovery.rs SHALL provide static integration evidence for integration test harness; named entrypoints include put, bridge, read, native_read_discovers_nested_rules_once_and_refires_after_compaction, main, failed_reads_and_oversized_rules_are_retried_after_repair, concurrent_real_path_access_delivers_one_reminder, initial_failure_is_not_hidden_by_a_seeded_directory, concurrent_different_directories_each_deliver_their_rules, native_list_write_and_streaming_read_all_use_the_same_discovery_boundary, scope_root_gitignore_nested_ignore_and_managed_denies_exclude_rules, unreadable_ignore_file_blocks_discovery_until_repaired, disabled_reminders_do_not_acknowledge_undelivered_rules, symlink_escape_in_targets_files_and_rule_directories_is_excluded, persistent_shell_cwd_is_discovered_without_parsing_command_paths, call_local_cwd_and_display_path_remapping_reach_real_files. The implementation exercises discover, filter, deduplicate, budget, and render skill/rule listings with retry state and explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; source review does not claim execution success.

#### Scenario: failed_reads_and_oversized_rules_are_retried_after_repair
- **WHEN** the test scenario failed_reads_and_oversized_rules_are_retried_after_repair is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: concurrent_real_path_access_delivers_one_reminder
- **WHEN** the test scenario concurrent_real_path_access_delivers_one_reminder is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: initial_failure_is_not_hidden_by_a_seeded_directory
- **WHEN** the test scenario initial_failure_is_not_hidden_by_a_seeded_directory is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

证据：`crates/codegen/tools/tests/agents_md_discovery.rs` — `put`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `bridge`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `read`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `native_read_discovers_nested_rules_once_and_refires_after_compaction`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `main`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `failed_reads_and_oversized_rules_are_retried_after_repair`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `concurrent_real_path_access_delivers_one_reminder`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `initial_failure_is_not_hidden_by_a_seeded_directory`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `concurrent_different_directories_each_deliver_their_rules`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `native_list_write_and_streaming_read_all_use_the_same_discovery_boundary`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `scope_root_gitignore_nested_ignore_and_managed_denies_exclude_rules`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `unreadable_ignore_file_blocks_discovery_until_repaired`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `disabled_reminders_do_not_acknowledge_undelivered_rules`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `symlink_escape_in_targets_files_and_rule_directories_is_excluded`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `persistent_shell_cwd_is_discovered_without_parsing_command_paths`；`crates/codegen/tools/tests/agents_md_discovery.rs` — `call_local_cwd_and_display_path_remapping_reach_real_files`。

### Requirement: Tools crates/codegen/tools/tests/cgroup_memory_test.rs integration test harness contract
crates/codegen/tools/tests/cgroup_memory_test.rs SHALL provide static integration evidence for integration test harness; named entrypoints include test_memory_config, make_request, is_linux_with_cgroupv2, can_create_cgroups, skip_unless_cgroup, print_result, test_under_limit_exits_normally, test_over_limit_gets_oom_killed, test_session_survives_oom, test_background_task_oom, test_gradual_allocation_oom, test_no_config_no_enforcement. The implementation exercises compose the tools crate public/module boundary and its explicit input/output/error paths and explicit markers filesystem or durable persistence、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; source review does not claim execution success.

#### Scenario: test_memory_config
- **WHEN** the test scenario test_memory_config is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: is_linux_with_cgroupv2
- **WHEN** the test scenario is_linux_with_cgroupv2 is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: test_under_limit_exits_normally
- **WHEN** the test scenario test_under_limit_exits_normally is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

证据：`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_memory_config`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `make_request`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `is_linux_with_cgroupv2`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `can_create_cgroups`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `skip_unless_cgroup`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `print_result`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_under_limit_exits_normally`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_over_limit_gets_oom_killed`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_session_survives_oom`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_background_task_oom`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_gradual_allocation_oom`；`crates/codegen/tools/tests/cgroup_memory_test.rs` — `test_no_config_no_enforcement`。

### Requirement: Tools crates/codegen/tools/tests/path_suggestions_production.rs integration test harness contract
crates/codegen/tools/tests/path_suggestions_production.rs SHALL provide static integration evidence for integration test harness; named entrypoints include literal, setup_fs, similar_names, pattern1_parent_exists_wrong_leaf_suggests_similar, pattern1_pluralization_typo, pattern1_missing_extension, pattern1_wrong_suffix, pattern1_parent_dir_itself_missing, pattern1_contributing_md_guess, pattern2_absolute_path_completely_different_tree, pattern2_grow_sessions_internal_path, pattern3_dropped_folder_with_display_remap, pattern3_dropped_folder_relative_path_skipped, pattern4_lib_vs_libs, pattern4_src_does_not_exist_no_misleading_suggestion, pattern5_root_file_with_close_match, pattern5_root_file_no_match, format_hallucinated_deep_path_with_similar. The implementation exercises compose the tools crate public/module boundary and its explicit input/output/error paths and explicit markers filesystem or durable persistence、explicit error classification、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; source review does not claim execution success.

#### Scenario: pattern2_absolute_path_completely_different_tree
- **WHEN** the test scenario pattern2_absolute_path_completely_different_tree is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: pattern3_dropped_folder_with_display_remap
- **WHEN** the test scenario pattern3_dropped_folder_with_display_remap is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: pattern3_dropped_folder_relative_path_skipped
- **WHEN** the test scenario pattern3_dropped_folder_relative_path_skipped is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

证据：`crates/codegen/tools/tests/path_suggestions_production.rs` — `literal`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `setup_fs`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `similar_names`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_parent_exists_wrong_leaf_suggests_similar`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_pluralization_typo`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_missing_extension`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_wrong_suffix`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_parent_dir_itself_missing`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern1_contributing_md_guess`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern2_absolute_path_completely_different_tree`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern2_grow_sessions_internal_path`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern3_dropped_folder_with_display_remap`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern3_dropped_folder_relative_path_skipped`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern4_lib_vs_libs`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern4_src_does_not_exist_no_misleading_suggestion`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern5_root_file_with_close_match`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `pattern5_root_file_no_match`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `format_hallucinated_deep_path_with_similar`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `format_hints_disabled_bare_error`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `format_no_match_just_cwd_note`；`crates/codegen/tools/tests/path_suggestions_production.rs` — `format_dropped_folder_shows_display_path`。

### Requirement: Tools crates/codegen/tools/tests/test_subagent_soak.rs integration test harness contract
crates/codegen/tools/tests/test_subagent_soak.rs SHALL provide static integration evidence for integration test harness; named entrypoints include DHAT_ALLOC, PARENT_SESSION_ID, Metric, label, str, summary_key, unit, budget, growth_in_budget_unit, expected_on_this_platform, MetricValue, value_of, serialize_metrics, bytes_to_mib, HeapSample, HeapMetrics, new, Bounds. The implementation exercises compose the tools crate public/module boundary and its explicit input/output/error paths and explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit; source review does not claim execution success.

#### Scenario: budget
- **WHEN** the test scenario budget is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: growth_in_budget_unit
- **WHEN** the test scenario growth_in_budget_unit is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

#### Scenario: serialize_metrics
- **WHEN** the test scenario serialize_metrics is executed
- **THEN** its assertions cover the integration test harness boundary; pass/fail must come from a later test run.

证据：`crates/codegen/tools/tests/test_subagent_soak.rs` — `DHAT_ALLOC`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `PARENT_SESSION_ID`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `Metric`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `label`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `str`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `summary_key`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `unit`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `budget`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `growth_in_budget_unit`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `expected_on_this_platform`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `MetricValue`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `value_of`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `serialize_metrics`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `bytes_to_mib`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `HeapSample`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `HeapMetrics`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `new`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `Bounds`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `from_env`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `Measurement`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `serialize_counts`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `Summary`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `heap_capture`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `quiesce`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `MAX_POLLS`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `SLEEP`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `SoakControl`；`crates/codegen/tools/tests/test_subagent_soak.rs` — `ProgressFuture`。

### Requirement: Tools crates/codegen/tools/tests/web_citation_counter.rs integration test harness contract
crates/codegen/tools/tests/web_citation_counter.rs SHALL provide static integration evidence for integration test harness; named entrypoints include web_citation_counter. The implementation exercises compose the tools crate public/module boundary and its explicit input/output/error paths and explicit markers tool definition, schema, or registry projection; source review does not claim execution success.

#### Scenario: Harness scope
- **WHEN** crates/codegen/tools/tests/web_citation_counter.rs is loaded by the test/build harness
- **THEN** its external prerequisites and static entrypoints are recorded without runtime claims.

证据：`crates/codegen/tools/tests/web_citation_counter.rs` — `Integration tests for the shared web citation counter.`。
### Requirement: Pager task-result test harness correlation and notification helper contract
The task-result test harness SHALL construct session-bound DoctorFixTarget snapshots, enqueue model control tokens, and serialize Grow session notifications through the ACP handler before asserting reducer state. These helpers define test evidence for identity correlation and authoritative projection; they do not prove runtime transport or compiled integration.

#### Scenario: Target identity snapshot
- **WHEN** a doctor result is dispatched after session identity changes
- **THEN** doctor_target captures agent id, session id, binding epoch, and cwd for stale-identity checks.

#### Scenario: Model control correlation
- **WHEN** a model switch result is synthesized
- **THEN** begin_model_control returns the local control token used to match the completion.

#### Scenario: Authoritative notification
- **WHEN** a Grow SessionUpdate is delivered
- **THEN** apply_grow_session_update serializes the notification and routes it through the ACP handler.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `doctor_target`；`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `begin_model_control`；`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `apply_grow_session_update`。
### Requirement: Nix OHOS vendored manifest and feature boundary
Cargo.toml SHALL declare vendored nix 0.26.4 on Rust 1.56 with the upstream package metadata, feature-gated Unix API modules, optional dependency relationships, five integration-test targets, target-specific dev/dependencies, and the local cfg(fbsd14) lint declaration. The manifest comments record the OHOS-as-musl compatibility patch, local ABI lint fixes, and the upgrade/removal condition.

#### Scenario: Nix OHOS vendored manifest and feature boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/Cargo.toml` — `[package]`；`third_party/nix-ohos/Cargo.toml` — `[package.metadata.docs.rs]`；`third_party/nix-ohos/Cargo.toml` — `[[test]]`；`third_party/nix-ohos/Cargo.toml` — `[dependencies.bitflags]`；`third_party/nix-ohos/Cargo.toml` — `[dependencies.cfg-if]`；`third_party/nix-ohos/Cargo.toml` — `[dependencies.libc]`；`third_party/nix-ohos/Cargo.toml` — `[features]`；`third_party/nix-ohos/Cargo.toml` — `default =`；`third_party/nix-ohos/Cargo.toml` — `[target.`；`third_party/nix-ohos/Cargo.toml` — `[lints.rust]`。

### Requirement: Nix upstream manifest provenance
Cargo.toml.orig SHALL preserve the normalized package provenance for nix 0.26.4, its upstream feature graph, target-specific dependencies, dev dependencies, and integration-test target declarations; it is a reference input and does not contain the generated-file local lint/patch notes.

#### Scenario: Nix upstream manifest provenance implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/Cargo.toml.orig` — `[package]`；`third_party/nix-ohos/Cargo.toml.orig` — `[package.metadata.docs.rs]`；`third_party/nix-ohos/Cargo.toml.orig` — `[features]`；`third_party/nix-ohos/Cargo.toml.orig` — `[dev-dependencies]`；`third_party/nix-ohos/Cargo.toml.orig` — `[[test]]`。

### Requirement: Nix MIT license resource
The vendored LICENSE resource SHALL state the MIT license terms for this third-party nix source tree.

#### Scenario: Nix MIT license resource implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/LICENSE` — `MIT license text`。

### Requirement: Nix platform and MSRV documentation
README.md SHALL document the purpose of safe Rust bindings over *nix APIs, supported platform tiers, the gethostname example, MSRV 1.56.1, contribution links, and MIT licensing; documentation is not runtime support evidence.

#### Scenario: Nix platform and MSRV documentation implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/README.md` — `# Rust bindings to *nix APIs`；`third_party/nix-ohos/README.md` — `## Supported Platforms`；`third_party/nix-ohos/README.md` — `## Minimum Supported Rust Version`；`third_party/nix-ohos/README.md` — `## License`。
