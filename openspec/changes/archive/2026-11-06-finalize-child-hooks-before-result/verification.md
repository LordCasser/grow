# Verification: finalize-child-hooks-before-result

当前状态：child正常完成已取得旧红新绿，最终五项终态actor和七项SessionAdmin回归、既有248项子任务取消/恢复回归均通过；默认CLI整合build通过后，真实runner正常/取消两个test及普通非leader退出test全部通过。普通退出暴露的独立Activity owner竞态已修复并随同新CLI验收，详见本change末尾最终记录。

## 已执行

| 检查 | 结果 |
| --- | --- |
| OpenSpec strict 校验 | `openspec validate finalize-child-hooks-before-result --strict --no-interactive` → `Change 'finalize-child-hooks-before-result' is valid`。 |
| 关键链核对 | `handle_request.rs` 收尾（artifact→result→usage→Shutdown→Ended→receipt）、`run_loop.rs` Shutdown 臂/`finish_graceful_shutdown` 全序、`hook_dispatch.rs` 空 plan 也写 `Triggered`、`timeline.rs::validate_header` `subagent_result_recorded` 封闭、`teardown.rs::final_session_persistence_flush` 排空段、取消（`ChildControl::cancel` 发 `Cancel`）与 panic（`runner_panicked` 发 Cancel+Shutdown 后走 `finalization.rs` 冷恢复）路径，均已亲核源码。 |
| 编译风险静态核对（无 Cargo） | `SessionCommand` 新变体的全部 match 位点：run_loop 主 match 新增专属臂；`sideband.rs`（goal usage mailbox）、`goal.rs`/`planning.rs`（非 SessionCommand）、`notification_bridge.rs`（带 `_ =>`）、`tests/support.rs` fixture（带 `_ => panic!`）、`storage/search.rs`（非 SessionCommand）——无遗漏穷举位点。类型/可见性（`chat_state::` 路径、`oneshot`、`use super::*` 链、私有项对子模块可见）逐一核对。 |
| rustfmt | `rustfmt --edition 2024`（写+`--check` 复核 clean）覆盖全部 8 个触碰源码/测试文件。 |
| 写边界自查 | 未触碰 `turn/{mod,settlement}.rs`、`subagent_usage_fold_tests.rs`、`rewind.rs`/rewind tests、`workspace/file_state`、sampler、`chat-state/timeline.rs` validator；`teardown.rs` 仅父授权的最小拆分。 |
| 父预审修正落盘 | ① commit 块移到 permission audit 检查之后（audit 失败不提交、不 ACK 成功，responder drop → runner unknown → 恢复）；② `handed_off` 在命令送达 mailbox 时即置位（`hand_off_child_result_to_actor` 新增 `&mut bool` 参数），外层 deadline 取消也能看到 actor 已接管 shutdown；③ patch 仅显式列本项文件（见下）。 |
| actor 测试 seed 前提修复（静态核对，无 Cargo） | 缺口：`create_test_actor_ex` 从不写 `SubagentSeed`，`SubagentResult` 在 `chat-state/src/timeline.rs` validator 必 `MissingSubagentSeed`（`startup_hints.is_subagent` 不能替代 durable seed）。修复：`terminal_commit_actor` 在 `run_session` 启动前经 `record_timeline_event_durably` 写入合法 seed（7 字段逐项对照 `prompt_queue.rs:2305` 既有 fixture；`subagent_id` 与 Result 一致以过 `SubagentSeedMismatch` 检查）。ACK 链核对：非 physical 模式下 support 桥（`tests/support.rs` `TimelineDurablyAndAck` 非 Forward 臂）自行 ACK durable Timeline 写，测试本地 stub 只需 ACK `FlushAndAck`（teardown `final_session_persistence_flush` 经桥 `other` 臂转发）。挂死排查：test actor `owns_permission_manager=false`、`permission_audit_bridge=None`；subagent 分支 goal settle/checkpoint 空转；drain/flush 均有界——shutdown 路径无等待已丢弃 ACK 的位点。失败面静态核对：到期 deadline 在 `persist_pending_timeline_event` 入口即返回 `DeadlineExceeded`（事件未 accept 进 in-memory timeline，故“无 Result”断言成立）；ACK 接收者 drop 时 run_loop `let _ = respond_to.send(..)` 不阻塞。断言消息所需 trait 核对：`TimelineWriteError`/`TimelineRangeRef` 均 derive Debug，`HookEventType::SessionEnd` 存在。 |
| 真实 Cargo 运行（父窗口 task 01a1af32-b962-7762-9d18-f996c8801dd3，日志 /tmp/grow-backlog-shell-terminals.log） | 25 项 24 过：`terminal_commit_records_session_end_before_the_last_canonical_result`、`cancelled_result_keeps_the_same_terminal_commit_order`、`dropped_ack_receiver_neither_stalls_the_actor_nor_duplicates_the_result` 通过（seed 修正生效）；`expired_deadline_appends_no_result_and_acks_the_typed_failure` 失败——DeadlineExceeded 使 ChatState writer 进入 poisoned 状态，actor 退出后 `timeline_events()` 返回 `None`，`subagent_terminal_commit_tests.rs:326` `unwrap()` panic。结论：失败路径不能以 in-memory 查询作持久化证据。 |
| 失败路径证据链修复 + G5 注入落盘（静态核对，无 Cargo） | fixture 改既有 `create_test_actor_ex_with_physical_projection`：每个 Timeline 写经本地 persistence stub，先记录成功事件进共享 `Vec<TimelineEvent>` 再 ACK Ok（记录先于 ACK，存活的持久证据）；expiry 测试改用 stub 记录证明（含 seed 阳性对照 + Result 数为 0），不再 unwrap poisoned 后的 `None`；正常/Cancelled/丢 ACK 保留内存断言并各补一条持久证据断言（恰好 1 个 Result）。新增注入：stub 仅对 SessionEnd Hook `Triggered` 写返回 `InvalidData`（`persistence_error_is_permanent` 命中 → 不重试）永久错误，seed 不失败；断言 Commit ACK 非 success（responder drop → runner unknown）、持久记录无 Result、无 Triggered、actor 在 watchdog（10s join timeout）内退役。生产代码与共享 support 未改。integration 文件 `test_subagent_session_end_order.rs:110` 按父指令单行适配 `let mut sandbox`（`set_env` 需 `&mut`，E0596 修复，该文件内 `set_env` 是唯一 mut 用点），其余断言未动。 |
| rustfmt（本轮） | `rustfmt --edition 2024`（写 + `--check` 复核 clean）覆盖 `subagent_terminal_commit_tests.rs`。 |

## 当前验收范围

五项默认 actor 测试通过真实 `run_session` 邮箱驱动 `CommitSubagentResult`，本地 physical persistence stub 先保存成功事实再 ACK。覆盖正常完成、Cancelled outcome、接收者丢失、到期 deadline 与 SessionEnd Hook 永久写入失败；最后两条以持久记录而非 poisoned 后内存查询为证。真实 runner 正常完成还验证外部 Hook marker、唯一最终 Result 与父 Ended 精确引用。实际命令与日志见下表。

## 真实 runner 收尾对照（fixture 修复后，本 coder Cargo 独占窗口）

夹具缺口与修复：`load_child_timeline`（`test_subagent_session_end_order.rs:64`）把 `grow_home/sessions/` 下常规索引文件当 cwd 目录 `read_dir` → ENOTDIR panic，此前两轮（old/new 日志 /tmp/grow-backlog-child-end-{old,new}.log）均失败于夹具而非顺序断言，不能当 red/green 证据。最小修复：外层 `cwd_entry` 增加 `file_type().expect(...).is_dir()` 检查，常规非目录跳过（`continue`），真实 `read_dir` 错误仍 `expect` 失败，未用 flatten 吞错或 empty fallback；内层 timeline.jsonl 过滤与全部断言未动。仅改该 helper，production/unit/support 未改。

| 运行 | 命令 | 结果 |
| --- | --- | --- |
| 旧基线（`GROW_BINARY=/tmp/grow-child-end-baseline-f0f461e54`） | `cargo +1.93.1 test --locked --offline -p shell --test test_subagent_session_end_order -- --ignored --nocapture --test-threads=1` | 真实 exit **101**（/tmp/grow-backlog-child-end-fix-old.log），失败点 `test_subagent_session_end_order.rs:237:18` panic —— 顺序断言红，非夹具。预期 red 确认。 |
| 新构建（`GROW_BINARY=target/debug/grow`，父预构建） | 同上 | 真实 exit **0**，1 passed（/tmp/grow-backlog-child-end-fix-new.log）。预期 green 确认。 |
| 冻结 unit/wire（只运行） | `cargo +1.93.1 test --locked --offline -p shell --lib -- subagent_terminal_commit_tests extensions::session_admin --test-threads=1` | 真实 exit **0**，12 passed; 0 failed（/tmp/grow-backlog-child-end-fix-units.log；含 5 项终态 actor 与 session_admin wire）。frozen 文件未改。 |

环境：`PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`，串行 `--test-threads=1`。exit 均取自命令自身 `$?`，无掩码。Cargo 窗口已释放。



- `cargo +1.93.1 test --locked --offline -p shell --lib agent::subagent:: -- --test-threads=1`：**248 passed / 0 failed**，日志 `/tmp/grow-backlog-subagent-recovery.log`。包含既有取消等待prompt终态、Cancelled canonical link、冷恢复拒绝live writer、usage/Ended/receipt幂等和Workflow历史恢复，不重新执行Hook。该组通过不替代本change真实runner取消对照。
- runner级取消与普通Shutdown均由下文最终新CLI产品链验证通过。未在Windows现场运行，不能把源码和本机mock回归当成现场根因或恢复结果。

## ordinary SessionEnd e2e 夹具补验（后续窗口）

父补验发现 `test_session_end_hook_e2e` 旧夹具未配置任何 provider/model，headless prompt 在到达 mock server 前即失败 `no LLM configured`、零请求（日志 /tmp/grow-backlog-ordinary-session-end.log）；同轮 `agent::subagent::` 全组 248 项独立通过，不能据此排除 ordinary 退出生产缺陷。最小修复：在 `run_with_session_end_hook` 中借同目录 `test_subagent_session_end_order.rs` 已成功的形状向 `sandbox.grow_home()/config.toml` 写入 isolated mock provider（`base_url = server.url()`、`env_key = "GROW_API_KEY"`、`default = "mock/headless"`；key 隔离仍由 `TestSandbox::builder().mock_url` 负责）。marker / stdin envelope / `reason = "shutdown"` 断言与 hook 脚本未动；共享 builder、业务代码未改。

| 项 | 状态 |
| --- | --- |
| `crates/codegen/shell/tests/test_session_end_hook_e2e.rs`（fixture：mock provider 配置 + 顶层 `--no-leader`；marker/stdin/reason 断言未动） | 已编辑，`rustfmt --edition 2024` clean。 |

### 运行记录（普通退出，exit 均为命令自身 `$?`）

| 运行 | 结果 |
| --- | --- |
| 补模型后（`/tmp/grow-backlog-ordinary-session-end-fixed.log`，父运行） | **测试命令 exit 101**；headless子进程本身exit 0，但 marker 缺失（`:120` panic）。未据此确认leader模式的因果关系，随后显式non-leader对照仍失败。 |
| 加 `--no-leader`（`/tmp/grow-backlog-ordinary-session-end-noleader.log`） | 真实 exit **101**，失败点不变：`assert_headless_success` 已过（prompt 成功、进程正常退出约 1.0s、`timed_out=false`），marker 断言 panic，stderr 空。`1 failed`。 |
| 诊断运行（`GROW_LOG_FILE`→`/tmp/grow-session-end-e2e-diag/`，`/tmp/grow-backlog-ordinary-session-end-diag4.log`，真实 exit 101） | 归因证据：hook **已注册加载**（`loaded from global source ... count=1`、actor `loaded hooks hook_count=1`、session_harness `hook_names:["global/session_end:session_end"]`）；prompt 成功（end_turn，modelCalls 2，mock 请求到达）；`shutdown: flushing session` 后紧跟 `WARN chat_state::actor: buffered Timeline commit failed; ... error=timeline persistence acknowledgement was lost seq=31 retry_delay_ms=25`，日志到此为止——**无任何 hook plan/trigger/SessionEnd 执行行，marker 未写**。初步指向普通退出 flush 在 SessionEnd 执行前被 Timeline 持久化 ACK 丢失重试截断；疑似真生产普通退出回归，已按边界仅报告父，未改业务代码。 |
| diag5（F3 candidate apply main 后） | 止步编译：`error[E0004]: non-exhaustive patterns: PromptCompletionKind::AdmissionReplay not covered`（shell lib）。F3 引入的变体缺 match 臂，非 ordinary 回归 fail；已原样报告父，待 F3 修复后补跑。 |



父补充源码定位与对照：已编译同一ordinary夹具直接运行旧CLI `f0f461e54`，一次 **1 passed / exit0**（`/tmp/grow-backlog-ordinary-session-end-baseline.log`）。`begin_graceful_shutdown` 在Hook/final flush前关闭cmd_rx，而AgentActivity以Sender::is_closed判定actor退役；这允许agent LocalSet提前退出并截断persistence。旧CLI一次通过不能否定该调度竞态。本项独立修复使用既有SessionThread观察actor与persistence实际退出，记录于协调design“进程退出的 Session owner 观察”，普通退出门禁已由末尾新CLI三条产品链验证通过。

`/tmp/finalize-child-hooks-before-result.patch`（git diff，显式 pathspec，仅含本项 6 个修改文件 + 3 个新增文件/目录；不含 worktree 预存的 backlog/archive 删除项）。禁止在本 worktree 外 apply。

## 取消场景（Tier 3, tasks 3.3）真实 runner — 已通过

子任务范围：只新增 `crates/codegen/shell/tests/test_subagent_session_end_order.rs` 中的取消 test（`cancelled_child_session_end_precedes_cancelled_result_through_real_runner`，`#[ignore]`，需新构建 GROW_BINARY）。同文件既有正常完成 test 未改动。

- 夹具：双隔离 mock server（parent 用脚本期望 + fallback 文本；child 独立 server `hold_agent_completions()` 保持真实入流不终结）；隔离 temp config（`mock/parent` → parent server，`child/live` → child server）；同文件既有 session_end hook 配置/marker/diagnostic 形状复用为 `install_session_end_hook`。
- 链路：prompt1 = parent todo_write + background `spawn_subagent`（真实 runner），从 parent server 实际 tool result 解析 `subagent_id`（不硬编码）；bounded 等待 child 真实入流（child server 出现 chat/completions 请求）且 child Timeline 存在、尚无 `SubagentResult`；prompt2 = `kill_command_or_subagent(task_id=<真实 child id>)` + `wait_subagent`，断言 wait 返回 `all_finished`/`status="cancelled"`。
- 断言：child Timeline 唯一 `SubagentResult(outcome=Cancelled)` 为最后事实；SessionEnd `Triggered`/`Completed` 各恰一次且都在 Result 前；父 `Ended` 恰一次、`outcome=Cancelled`、`result_ref` 精确指向 child timeline 的该 Result seq；外部 hook marker ≥1；diagnostic 无 `session-end hook lifecycle was not durable` 且无 `SubagentTimelineEnded`。

最终父整合：`cargo +1.93.1 build --locked --offline -p cli --bin grow` exit0，日志 `/tmp/grow-backlog-cli-integrated-build.log`。随后设 `GROW_BINARY=target/debug/grow`，运行 `cargo +1.93.1 test --locked --offline -p shell --test test_subagent_session_end_order --test test_session_end_hook_e2e -- --ignored --nocapture --test-threads=1`，**exit0**，真实runner正常和取消 **2 passed**，普通非leader退出 **1 passed**。完整日志 `/tmp/grow-backlog-terminal-e2e-integrated.log`，父亲核输出与取消/引用断言。

同一编排中的Shell库测试当轮因F3新增fixture借用已move的字符串而编译失败，不能将编排整体称为全绿；该编译错误不在上述独立e2e测试命令中，三条产品链确已执行。F3修正与ACP验收单独记录。Windows现场未运行；不把本机mock回归解释为用户现场根因已复现。
