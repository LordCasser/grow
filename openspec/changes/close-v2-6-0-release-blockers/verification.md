# v2.6.0 最终本机验证记录

本记录以最终候选本机日志为准。较早的失败和修复过程保留在下面的“历史失败”中；它们不替代最终通过结果，也不抹去仍未运行的 CI/发布门禁。

清理前 `openspec/backlog.md` 的原样快照（八项非延期条目与启动基线一致）保存在 [`backlog-before.md`](backlog-before.md)，SHA-256 `3b48c6e00199ab3a5887a2e3e14e0324fdcfea38246dc9647db52fab08e9bb2f`；复制前后哈希一致。

## 最终本机门禁

| 门禁 | 结果 | 证据与边界 |
| --- | --- | --- |
| 18-crate core suite | 19233 passed / 0 failed / 51 ignored | `/tmp/grow-260-final-core5.log`；18 个 `Running unittests` block 各自最终 summary 全部通过。命令：`cargo test --locked --no-fail-fast --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -p agent -p tools -p tool-types -p diagnostics -p pager-render -p tty-utils -p test-support -p fsnotify -p hunk-tracker -p workspace -- --test-threads=4`。 |
| Responses PTY | parallel 1/1（6.57s）；single 1/1（8.06s） | `/tmp/grow-260-responses-parallel-final.log`、`/tmp/grow-260-responses-single-final.log`；均以最终 CLI 构建并运行精确测试。parallel caller 已加入 `.github/workflows/core-regression.yml`。 |
| Protocol PTY | 3 × 1 passed / 0 failed | `/tmp/grow-260-final-goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates8.log`、`/tmp/grow-260-final-malformed_wire_json_allows_manual_continuation_in_the_same_resident_session8.log`、`/tmp/grow-260-final-protocol_failure_preserves_session_across_continue_switch_and_process_restart8.log`。 |
| Settings E2E | 235 passed / 0 failed | `/tmp/grow-260-final-settings8.log`。 |
| Workspace check | PASS | `/tmp/grow-260-final-workspace8.log`：1m30s，无 warning；最终增量复核 `/tmp/grow-260-final-workspace-last.log` 也 PASS。 |
| Formatting | PASS | `/tmp/grow-260-final-format.log`，无诊断。 |
| Active OpenSpec strict validation | 26 passed / 0 failed | `/tmp/grow-260-final-strict-prearchive.log`，pre-archive。 |
| Archived OpenSpec strict validation | 836 passed / 0 failed | 最终归档后复核 `/tmp/grow-260-postarchive-archived-final.log`；归档前的 828/828 由此更新。 |
| Post-archive active/archived validation | active 18/18；archived 836/836 | `/tmp/grow-260-postarchive-active-final.log`、`/tmp/grow-260-postarchive-archived-final.log`。 |
| Release-script tests | 34 tests：33 passed / 1 skipped | `/tmp/grow-260-final-scripts-last.log`；这是脚本测试，不是 `scripts/validate-release.sh v2.6.0` 发布执行。 |
| Ordinary local release build | PASS，3m21s | `/tmp/grow-260-final-release8.log`。该次为普通本地 `release` build，不是官方 `release-dist`。 |

本机 `target/release/grow` 是 Mach-O arm64，版本显示 `grow 2.6.0 (bbd2c4dfa) [alpha]`；`codesign --verify --strict` 通过，SHA-256 为 `31348dd8394ffa801197767fc3947d44dc44ff5c23a07a8ba3ae53f3128c4342`。该二进制来自未提交的源码树，只是本机候选检查产物，不能作为正式发布 asset。

## 八项闭合矩阵

“本机证据完成”不等于跨平台 CI 或正式发布完成。第 3 项仍有 native CI 测试范围；ProcessScope 身份并发证明继续暂缓；全局 RAM cap 没有设定。

| # | Backlog 基线项 | Owner change | 最终本机证据 | 结论和剩余边界 |
| --- | --- | --- | --- | --- |
| 1 | macOS 配置监听与 MCP 门禁 | [offload-dynamic-config-watch-updates](../archive/2026-11-04-offload-dynamic-config-watch-updates/verification.md) | Pager MCP target 14/14（`/tmp/grow-260-pager7-target-0.log`）；最终 Shell suite 中 20 个 watcher 用例全通过；4 个 ProjectDiscovery startup owner 测试通过（`/tmp/grow-260-final-target-project_discovery_startup_tests5.log`）。 | 本机 owner、注册/取消、late directory/atomic replacement 和 MCP 证据完成；Linux/Windows/macOS native CI 尚未运行。 |
| 2 | 原树出生权限 cold-load/retirement | [offload-dynamic-config-watch-updates](../archive/2026-11-04-offload-dynamic-config-watch-updates/verification.md) | 最终 Shell suite 4576 passed / 0 failed / 9 ignored；`suspended_child_cold_restore_retains_birth_authority_end_to_end`、`cold_resume_preserves_durable_effort_chain`、project-rules restore 和 task-resume tests 均为 `ok`，见 `/tmp/grow-260-final-core5.log`。 | 等待 owner 修复后原权限事实链通过；未放宽 birth capability。原 exact/full 本机回归通过；native CI 仍待跑。 |
| 3 | 子任务观察跨层边界和 reader panic | [close-subagent-observation-boundary-tests](../close-subagent-observation-boundary-tests/verification.md)、[catch-subagent-output-worker-panics](../archive/2026-11-04-catch-subagent-output-worker-panics/verification.md) | 最终 Shell suite 包含 3 个 actor/ChatState/cancel-receipt boundary tests，均通过；Tools observation group 4/4 通过，完整 Tools crate 2441 passed / 0 failed / 7 ignored。 | 本机验证完整；所需 macOS/Linux/Windows native CI 尚未运行。active forgotten ID 与 retired checkpoint、owner unavailable 与 empty inventory 分开断言。 |
| 4 | 外部变异测试资源策略与异常收尾 | [analyze-command-resource-budget-and-cleanup](../archive/2026-11-04-analyze-command-resource-budget-and-cleanup/verification.md)、[finish-command-groups-before-leader-reap](../archive/2026-11-04-finish-command-groups-before-leader-reap/verification.md) | disposable-copy interruption script PASS（`/tmp/grow-260-isolated-interruption.log`）；terminal bounded descendant target 1/1；最终 Tools suite 2441/0/7。 | 不设全局 RAM cap；高资源试验预算须由宿主/调用方提供。**ProcessScope `kill_all` 已 upgrade Weak 与 leader reap 并发时的身份同步仍在[明确暂缓](../../backlog.md)中**，审查细节见[review](../archive/2026-11-04-finish-command-groups-before-leader-reap/review.md)；native CI 仍待跑。 |
| 5 | 辅助指定模型实际路由 | [resolve-auxiliary-suggestion-model-routes](../archive/2026-11-04-resolve-auxiliary-suggestion-model-routes/verification.md) | 最终 Shell suite 中三入口 endpoint/backend/credential route test 通过；AI suggestion sideband test 覆盖 Chat Completions、Responses、Messages 并通过。 | 本机路由验证完成；官方多平台 CI 仍待跑。 |
| 6 | Responses 单工具与 parallel PTY fixtures | [fix-pty-responses-tool-stream-fixtures](../archive/2026-11-04-fix-pty-responses-tool-stream-fixtures/verification.md) | 两个真实 Responses PTY 精确用例分别 1/1 通过；parallel case 保留 HTTP tools、双 outputs、双文件 marker 和合并行断言，且已加入 Core regression workflow。 | 本机最终 CLI 通过；workflow CI 尚未执行。temporary workspace 使用真实 Git repository。 |
| 7 | 临时目录 skipped/written 回执 | [distinguish-skipped-memory-log-writes](../archive/2026-11-04-distinguish-skipped-memory-log-writes/verification.md) | Memory crate 302/0，storage subset 83/0；ephemeral flush checkpoint 与 session-end receipt tests 均通过，见最终 core 和对应 target logs。 | storage、flush 和 session-end 本机证据完成；native CI 仍待跑。 |
| 8 | task cancellation entry scopes | [separate-task-cancel-entry-scopes](../archive/2026-11-04-separate-task-cancel-entry-scopes/verification.md) | Tools scoped cancellation 21/21；真实 SessionActor dispatch durable receipt test 在最终 Shell suite 通过。not-found 只表示该 ID 未找到，不虚构整个 child inventory 为空。 | 本机行为与 receipt 证据完成；native CI 仍待跑，归档后的规范合并在 OpenSpec archive 中检查。 |

## 历史失败与最终结果的关系

- 初期 Shell watcher target 为 19/20，失败来自 same-time replacement fixture 的临时路径 alias；fixture canonicalize 后，最终 Shell core 中 watcher 命名组 20/20。初期 Pager MCP group 曾有 10 项失败；最终 targeted MCP 组 14/14。
- Birth-authority cold-load 曾在十秒 fixture deadline 内挂起；后续追踪到 SessionActor 启动的 ProjectDiscovery native watcher 阻塞 command ACK。先把 worker 归到 per-session runtime 又暴露 session writer retirement 超时；再发现 host runtime shutdown 中 observer 未取消会导致 worker 留在 `Handle::block_on`。最终改为 host-runtime guardian 持有 cancellation guard。最终 exact birth-authority test 和 Shell core 均通过，权限未放宽。
- Subagent observation 旧 fixture 将发现结果误限为 `forgotten-active`，而公开 discovery 还返回 `retired-suspended-child`；fixture 与契约对齐后最终 4/4 通过。早期 cancel receipt fixture 因没有 active turn 等测试搭建问题失败，修正后最终 actor durable receipt 测试通过。
- Auxiliary route 早期测试因 workflow sampler context window 为零失败，fixture 修正后最终 route/sideband 用例通过。早期 core run 中 prompt size、watcher 和 session retirement 失败也由后续收敛修复；`final-core5` 最终为 19233/0/51。
- Resources investigation 中曾有未运行的临时 terminal probe，已删除；正式 bounded descendant regression 现已纳入 production tests。百 GB 级实验没有重跑，也没有被中断脚本 PASS 伪装成产品资源上限。

## 尚未完成的最终门禁

本机 tests、workspace/format、settings、active/archive strict OpenSpec 和普通 release build 已通过。仍未完成 native 平台 CI、clean committed HEAD 与 annotated tag 一致性检查、官方十平台构建/资产 provenance，以及 `bash scripts/validate-release.sh v2.6.0`。正式发布授权已存在，但这些条件未满足前不能把候选标成已发布，也不能把本地二进制用作发布资产。
