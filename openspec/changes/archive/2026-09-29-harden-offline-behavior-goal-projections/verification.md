# 验证记录

环境：2026-09-29，macOS arm64，本地工作区。使用独立 `CARGO_TARGET_DIR=/tmp/grow-behavior-replay-target`，`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4`，不清理共享默认 target，不修改其他任务的执行侧代码。

## 验证范围

本轮先核对 Behavior/Goal 契约、Actor 生产者、实时 Pager 与离线消费者，再补齐只读展示。完整矩阵见 [audit.md](audit.md)。来源测试使用实际序列化的 Timeline Control 与 updates；播放器测试使用真实 Player/TranscriptProjection；执行侧测试仅作回归，未改执行状态机。

## 自动验证

使用 `cargo test --locked -p pager -p shell --lib --no-run` 生成 libtest 可执行文件，随后用以下 filter 运行。多 filter 为并集，每个测试只计一次。

| 范围 / filter | 结果 |
| --- | --- |
| Pager `transcript_projection` | 14 passed，含 7 个本轮新增用例 |
| Pager `replay_cmd::tests` | 16 passed，含 2 个本轮新增组合用例 |
| Pager `export_cmd::tests` | 6 passed，含目录树及 trajectory tar parity |
| Shell `session::storage::transcript::tests` | 20 passed，含 4 个本轮新增来源用例 |
| Shell `session::control::tests` | 14 passed |
| Shell `session::behavior::tests` | 26 passed |
| Shell `session::goal_tracker::tests` | 24 passed |
| Shell `session::actor::goal` | 32 passed（含 Goal admission 与 goal_support） |
| Pager `app::acp_handler::tests::{goals,plan_mode,session_events,workflows}` | 72 passed |
| Shell `actor::updates::{grow_event_id_stamping_tests,synthetic_prompt_behavior_tests}` 与 `actor::tool::{plan_finish_projection_tests,plan_mode_edit_gate_tests,state_control_batch_tests}` | 35 passed |

共 259 个不重复的定向测试；没有运行整个 workspace。初轮投影测试中“数字格式”和“clear 删除历史 Workflow 行”的错误预期已按实际展示契约修正；没有删除失败场景。链接器有现有大型 Rust 可执行文件的 `__eh_frame` 超过 16 MiB 警告，不影响通过结果。

## Delta 场景对应

| 场景 | 验证证据 |
| --- | --- |
| Goal changes during a foreground turn | projection 的正文 EntryId/AB 连续性；Player 在流式中切 Goal/Normal/clear 后正文仍部分显示；暂停冻结；0.5×/1×/8× 的已记录续轮、取消、退出、新输入与导出一致 |
| Goal is cleared or replaced | 空 ID clear 删除唯一当前 Goal；旧 identity 不复活；新目标替换；同 ID paused 后 active 重启 |
| Goal stops and late usage arrives | paused/blocked/budget_limited/complete 各自保留停止状态、累计下界和无预算；只取消 turn 的中间 tick 仍显示 active Goal |
| Plan selection needs confirmation or is rejected | 四 phase；confirmation 保留当前 Plan；控制 applied/rejected receipt 去重，transient 不变成交互或切换到 desired |
| Workflow update is stale or Behavior has changed | 旧/重复正 revision、旧 clear、clear 后零 revision 都不能回退；一个 Run 保留原 EntryId；Behavior 切换不终止 Run |
| Display cache omits the last Goal transition | 实际 Timeline 的 paused/Normal 或 cleared/Normal 修复 stale active cache；新增尾部事件 timestamp=None、simulated_source=true，正文顺序和源文件字节不变；匹配 cache 无重复 |
| Control is invalid | 实际序列化 Timeline 改为错误 architecture、revision、Active Goal/Normal ownership 后明确拒绝 |

## CLI 与终端

本目录 [verify_cli.py](verify_cli.py) 在隔离 Grow home 创建合成 display fixture，运行真实 `grow export` 与 PTY `grow replay --speed 100`。覆盖五种 Behavior（Clarify 的 wire ID 是 `ask`）、Goal active/paused/clear 后旧更新、Plan phase、打断后新输入、旧 terminal 去重、pending 工具、已有导出目录保护、粘贴隔离和退出时终端恢复。脚本检查 session 文件 SHA-256 不变，历史 `touch` 命令不产生 sentinel。

运行方式：`python3 openspec/changes/harden-offline-behavior-goal-projections/verify_cli.py /absolute/path/to/grow`；归档后使用 archive 下的同名文件。该 PTY fixture 的 Timeline 为空；canonical Control 与实际读取校验由上述 Rust 来源测试覆盖，不把合成终端测试当作真实 provider 执行。初次 CLI fixture 把 Clarify 写成非 wire ID、直接匹配带 ANSI 光标移动的状态栏字符串，已修正 fixture 和终端断言。

## 限制

截点恢复不能还原未保存的中间状态时间、审批模态框或按键。Workflow 缺失展示缓存的完整重建、跨 session 原子捕获、极限负载、多进程故障注入与 Windows/Linux 终端未在本轮验证。GoalUpdated 没有 definition/control revision，不能仅靠时间戳证明任意畸形混合日志的同 ID 更新顺序。实时 Workflow clear 的旧 revision 问题已单列 backlog。

## 收尾

`cargo build --locked -p cli --bin grow` 与最终 libtest 构建通过。追加的旧 clear 和取消后 Goal 仍 Active 中间态断言补齐后，projection + Player 共 30 项复验通过；不重复计入上面的 259 项。最终 CLI/PTY 脚本通过；改动 Rust 文件的 `rustfmt --edition 2024 --config skip_children=true --check`、`git diff --check` 和归档前 `openspec validate --all --strict --no-interactive` 均通过（15 items）。

已使用该独立 target 的 `cargo clean` 删除 14522 个文件、8.7 GiB；没有清理工作区默认 target。没有提交或回滚共享工作区中的其他改动。

已归档为 `2026-09-29-harden-offline-behavior-goal-projections`，两条新增 requirement 合入 client-surfaces。归档后 `openspec validate --all --strict --no-interactive` 为 14 passed，`openspec validate --archived --no-interactive` 为 556 passed，`git diff --check` 通过。
