# v2.6.0 最终发布验证

v2.6.0 已正式发布。下方先记录正式 tag、CI 与 GitHub Release 的核验结果；其后本机和旧候选记录保留为过程证据，不能覆盖最终发布状态。

清理前 `openspec/backlog.md` 的原样快照（八项非延期条目与启动基线一致）保存在 [`backlog-before.md`](backlog-before.md)，SHA-256 `3b48c6e00199ab3a5887a2e3e14e0324fdcfea38246dc9647db52fab08e9bb2f`；复制前后哈希一致。快照内的相对链接以原始 `openspec/backlog.md` 所在目录解释，归档时不改写快照字节。

## 正式发布结果

正式 tag `v2.6.0` 是 annotated tag，解析到 commit `5475dff997f05cf6c67ab8ba24621ac8863c144a`。发布预检时本机工作树 clean，HEAD 与 tag 一致；`/tmp/grow-260-official-preflight.log` 中的 `scripts/validate-release.sh v2.6.0` preflight 通过。

官方 [release workflow run 41709770374](https://github.com/LordCasser/grow/actions/runs/41709770374) 最终成功（attempt 2）。十个平台的 build 均在 attempt 1 成功；Native macOS/Linux/Windows、Windows storage 和 OpenSpec 门禁成功。Core 的成功 job [126219726206](https://github.com/LordCasser/grow/actions/runs/41709770374/job/126219726206) 命中 35 个精确通过标记：3 个 protocol 和 32 个选定 PTY 用例。Publisher job [126243448544](https://github.com/LordCasser/grow/actions/runs/41709770374/job/126243448544) 成功。

GitHub Release [v2.6.0](https://github.com/LordCasser/grow/releases/tag/v2.6.0) 的 release id 为 `433278862`，发布时间 `2026-11-03T22:20:21Z`（本地时间 11 月 4 日 06:20）；`draft=false`、`prerelease=false`，`/releases/latest` 指向 `v2.6.0`。资产为 10 个平台 tarball 加 `SHA256SUMS`，共 11 个文件、`461745882` bytes。独立下载清单核对了十项顺序和名称，每个条目都匹配 GitHub API digest，清单自身 digest 也匹配。证据文件为 `/tmp/grow-260-official-validation-41709770374/release.json` 和 `/tmp/grow-260-official-validation-41709770374/SHA256SUMS`。

首次官方 Core job [126174838438](https://github.com/LordCasser/grow/actions/runs/41709770374/job/126174838438) 的 Minimal Ctrl+D 测试在 5 秒观察 deadline 时仍显示 `Running`，总耗时 5.43 秒后失败。本机对同一二进制重复十次均通过（`/tmp/grow-260-minimal-quit-repeats.json`）；没有找到按键合并或丢弃的证据。`wait_exit_code` 不会 pump PTY，旧屏幕状态不足以判定挂起原因，因此该次超时原因仍未证实。相同 SHA 的一次诊断复验中 35 个用例全部通过；这不是源码或 tag 修复的证据。正式流程之前，候选 CI `5475dff` 的 Core [41705268335](https://github.com/LordCasser/grow/actions/runs/41705268335)、Native [41705268383](https://github.com/LordCasser/grow/actions/runs/41705268383) 和 OpenSpec [41705268313](https://github.com/LordCasser/grow/actions/runs/41705268313) 均成功。

本机 18-crate core 结果为 19233 passed / 0 failed / 51 ignored，来自源码 `fccd91b10e141d59d4ed87ca85629015a2949e55`；tag 的生产源码与该版本相同，之后只有两个 PTY 测试文件不同。最终发布门禁通过。Backlog 保留 9 项明确延期；用户已确认不新增全局 RAM cap，ProcessScope identity 同步仍按 backlog 暂缓。

发布后的独立复核还确认 GitHub Release 正文与 tag 中的 `crates/codegen/shell/changelogs/2.6.0.md` 一致。`git diff --name-only fccd91b10e141d59d4ed87ca85629015a2949e55 v2.6.0 -- Cargo.toml Cargo.lock crates scripts .github` 只列出两个已记录的 PTY 夹具，生产源码与构建配置未变。

## 最终本机门禁

| 门禁 | 结果 | 证据与边界 |
| --- | --- | --- |
| 18-crate core suite | 19233 passed / 0 failed / 51 ignored | `/tmp/grow-260-candidate3-core18.log`；对应源码 `fccd91b10e141d59d4ed87ca85629015a2949e55`，与正式 tag 的生产源码一致，之后只有两个 PTY 测试文件不同。命令：`cargo test --locked --no-fail-fast --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -p agent -p tools -p tool-types -p diagnostics -p pager-render -p tty-utils -p test-support -p fsnotify -p hunk-tracker -p workspace -- --test-threads=4`。 |
| Responses PTY | parallel 1/1（6.57s）；single 1/1（8.06s） | `/tmp/grow-260-responses-parallel-final.log`、`/tmp/grow-260-responses-single-final.log`；均以最终 CLI 构建并运行精确测试。parallel caller 已加入 `.github/workflows/core-regression.yml`。 |
| Protocol PTY | 3 × 1 passed / 0 failed | `/tmp/grow-260-final-goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates8.log`、`/tmp/grow-260-final-malformed_wire_json_allows_manual_continuation_in_the_same_resident_session8.log`、`/tmp/grow-260-final-protocol_failure_preserves_session_across_continue_switch_and_process_restart8.log`。 |
| Settings E2E | 235 passed / 0 failed | `/tmp/grow-260-final-settings8.log`。 |
| Workspace check | PASS | `/tmp/grow-260-final-workspace8.log`：1m30s，无 warning；最终增量复核 `/tmp/grow-260-final-workspace-last.log` 也 PASS。 |
| Formatting | PASS | `/tmp/grow-260-final-format.log`，无诊断。 |
| Active OpenSpec strict validation | 16 passed / 0 failed | `/tmp/grow-260-publication-active.log`；发布闭合项归档后，15 份规范与 1 个明确延期的活动提案通过。 |
| Archived OpenSpec strict validation | 844 passed / 0 failed | `/tmp/grow-260-publication-archived.log`；包含本轮全部完成归档。 |
| Release-script tests | 34 tests：33 passed / 1 skipped | `/tmp/grow-260-final-scripts-last.log`；这是脚本测试，不是 `scripts/validate-release.sh v2.6.0` 发布执行。 |
| Ordinary local release build | PASS，3m21s | `/tmp/grow-260-final-release8.log`。该次为普通本地 `release` build，不是官方 `release-dist`。 |

本机 `target/release/grow` 是 Mach-O arm64，版本显示 `grow 2.6.0 (bbd2c4dfa) [alpha]`；`codesign --verify --strict` 通过，SHA-256 为 `31348dd8394ffa801197767fc3947d44dc44ff5c23a07a8ba3ae53f3128c4342`。该二进制来自未提交的源码树，只是本机候选检查产物，不能作为正式发布 asset。

## 八项闭合矩阵

八项非延期工作均已通过本机证据和正式发布门禁。官方 run 41709770374 的 ten-platform build、Native 三平台、Windows storage、OpenSpec、Core exact markers 与 Publisher 均成功。ProcessScope 身份并发证明继续暂缓；全局 RAM cap 没有设定。

| # | Backlog 基线项 | Owner change | 最终本机证据 | 结论和剩余边界 |
| --- | --- | --- | --- | --- |
| 1 | macOS 配置监听与 MCP 门禁 | [offload-dynamic-config-watch-updates](../2026-11-04-offload-dynamic-config-watch-updates/verification.md) | Pager MCP target 14/14；本机 watcher 与 ProjectDiscovery startup owner 测试通过；官方 Native 三平台、ten-platform builds 和发布门禁成功。 | 注册/取消、late directory/atomic replacement、MCP 与正式跨平台构建证据完成。 |
| 2 | 原树出生权限 cold-load/retirement | [offload-dynamic-config-watch-updates](../2026-11-04-offload-dynamic-config-watch-updates/verification.md)、[await-parent-mcp-before-cold-child-grant](../2026-11-04-await-parent-mcp-before-cold-child-grant/verification.md) | 本机出生权限、effort、project-rules restore 和 task-resume tests 通过；初始化竞态修复后，官方 Core、Native 与十平台发布门禁成功。 | 原权限事实链通过，未放宽 birth capability；关键修复是冷恢复等待父 MCP 初始握手和发布完成后再采 grant。 |
| 3 | 子任务观察跨层边界和 reader panic | [close-subagent-observation-boundary-tests](../2026-11-04-close-subagent-observation-boundary-tests/verification.md)、[catch-subagent-output-worker-panics](../2026-11-04-catch-subagent-output-worker-panics/verification.md) | 本机 actor/ChatState/cancel-receipt tests 与 Tools observation 4/4 通过，完整 Tools crate 2441/0/7；官方 Core、Native 与十平台发布门禁成功。 | active forgotten ID 与 retired checkpoint、owner unavailable 与 empty inventory 分开断言；panic 隔离限于 unwind，release-dist 的 panic=abort 不承诺进程存活。 |
| 4 | 外部变异测试资源策略与异常收尾 | [analyze-command-resource-budget-and-cleanup](../2026-11-04-analyze-command-resource-budget-and-cleanup/verification.md)、[finish-command-groups-before-leader-reap](../2026-11-04-finish-command-groups-before-leader-reap/verification.md) | disposable-copy interruption script PASS；terminal bounded descendant target 1/1；官方 Core、Native 与十平台发布门禁成功。 | 用户于 2026-11-04 明确确认不新增全局 RAM cap；高资源试验预算须由宿主/调用方提供。**ProcessScope `kill_all` 已 upgrade Weak 与 leader reap 并发时的身份同步仍在[明确暂缓](../../../backlog.md)中**，审查细节见[review](../2026-11-04-finish-command-groups-before-leader-reap/review.md)。 |
| 5 | 辅助指定模型实际路由 | [resolve-auxiliary-suggestion-model-routes](../2026-11-04-resolve-auxiliary-suggestion-model-routes/verification.md) | 本机三入口 endpoint/backend/credential route tests 通过；AI suggestion sideband 覆盖 Chat Completions、Responses、Messages 并通过；官方 Core、Native 与十平台发布门禁成功。 | 本机路由和正式发布验证完成。 |
| 6 | Responses 单工具与 parallel PTY fixtures | [fix-pty-responses-tool-stream-fixtures](../2026-11-04-fix-pty-responses-tool-stream-fixtures/verification.md) | 两个真实 Responses PTY 精确用例本机各 1/1；parallel 保留 HTTP tools、双 outputs、双文件 marker 和合并行断言；官方 Core exact PTY markers 与十平台发布门禁成功。 | Core 最终复验的 32 个选定 PTY 用例通过；首次同 SHA Minimal Ctrl+D 超时原因仍未证实，见下方历史记录。 |
| 7 | 临时目录 skipped/written 回执 | [distinguish-skipped-memory-log-writes](../2026-11-04-distinguish-skipped-memory-log-writes/verification.md) | Memory crate 302/0，storage subset 83/0；ephemeral flush checkpoint 与 session-end receipt tests 本机通过；官方 Core、Native 与十平台发布门禁成功。 | storage、flush、session-end 与正式发布证据完成。 |
| 8 | task cancellation entry scopes | [separate-task-cancel-entry-scopes](../2026-11-04-separate-task-cancel-entry-scopes/verification.md) | Tools scoped cancellation 21/21；真实 SessionActor durable receipt test 本机通过；官方 Core、Native 与十平台发布门禁成功。not-found 只表示该 ID 未找到，不虚构整个 child inventory 为空。 | 本机行为、receipt 与正式发布证据完成。 |

## 历史失败与最终结果的关系

- 初期 Shell watcher target 为 19/20，失败来自 same-time replacement fixture 的临时路径 alias；fixture canonicalize 后，最终 Shell core 中 watcher 命名组 20/20。初期 Pager MCP group 曾有 10 项失败；最终 targeted MCP 组 14/14。
- Birth-authority cold-load 曾在十秒 fixture deadline 内挂起；后续追踪到 SessionActor 启动的 ProjectDiscovery native watcher 阻塞 command ACK。先把 worker 归到 per-session runtime 又暴露 session writer retirement 超时；再发现 host runtime shutdown 中 observer 未取消会导致 worker 留在 `Handle::block_on`。最终改为 host-runtime guardian 持有 cancellation guard。最终 exact birth-authority test 和 Shell core 均通过，权限未放宽。
- Subagent observation 旧 fixture 将发现结果误限为 `forgotten-active`，而公开 discovery 还返回 `retired-suspended-child`；fixture 与契约对齐后最终 4/4 通过。早期 cancel receipt fixture 因没有 active turn 等测试搭建问题失败，修正后最终 actor durable receipt 测试通过。
- Auxiliary route 早期测试因 workflow sampler context window 为零失败，fixture 修正后最终 route/sideband 用例通过。早期 core run 中 prompt size、watcher 和 session retirement 失败也由后续收敛修复；`final-core5` 最终为 19233/0/51。
- Resources investigation 中曾有未运行的临时 terminal probe，已删除；正式 bounded descendant regression 现已纳入 production tests。百 GB 级实验没有重跑，也没有被中断脚本 PASS 伪装成产品资源上限。

## 历史状态：455bcb 候选阶段尚未完成的门禁

在 455bcb 候选阶段，本机完整 core 有候选3证据，OpenSpec 与 Native 三平台 CI 已通过，但 Core CI 有一项 PTY 失败；clean HEAD/tag preflight、官方十平台构建与发布尚未执行。随后测试夹具修正通过候选5475 CI，正式 preflight、release workflow 与资产核验均已完成，见本文开头的正式发布结果。

提交核对包含所有新增归档文件；四份历史文本产物仅移除行尾空白/多余 EOF 空行，保留测试结果和正文。`git diff --check` 的最终检查覆盖完整候选 diff。

## 历史 CI：首轮候选 GitHub Core

首轮候选提交 `219220f4e`（随后文档提交 `b745d06b67434f04dc82c96ce03b5360f9a80cc2`）已推送到 `main`。GitHub [Core workflow run 41672827612](https://github.com/LordCasser/grow/actions/runs/41672827612) 的 [job 126045425349](https://github.com/LordCasser/grow/actions/runs/41672827612/job/126045425349) 失败，唯一失败为 Pager `header_new_agent_button_focused_is_green`：Pager 7567 passed / 1 failed / 15 ignored，后续步骤因该失败 skipped。CI excerpt 保存在 `/tmp/grow-260-candidate-core-ci-failure-excerpt.log`。

根因定位为 fixture 的状态与断言不符：`state.list_focused == false`，fixture 却期待 header focus 使用绿色。原本机环境 `NO_COLOR=1 TERM=dumb` 会让 `Theme::current` 将绿色和灰色均解析成 `Reset`，掩盖颜色断言差异；去除 `NO_COLOR` 并设置 `COLORTERM=truecolor TERM=xterm-256color` 运行原 binary 得到 0 passed / 1 failed，记录在 `/tmp/grow-260-dashboard-focus-before.log`。因此本机 `final-core5` 通过不能替代彩色环境的这条 CI 证据。

已完成并归档 [align-dashboard-header-focus-test](../2026-11-04-align-dashboard-header-focus-test/verification.md)，fixture 使用可区分颜色的固定 `Theme::grownight()`。彩色环境 Pager 全组 7566 passed / 0 failed / 15 ignored；最终 header 4 项通过。该修复仅调整夹具和陈旧注释，不改生产输入/渲染行为。

同一轮平台状态：Windows storage run [41672827626](https://github.com/LordCasser/grow/actions/runs/41672827626) 成功；Linux native job [126045425871](https://github.com/LordCasser/grow/actions/runs/41672827694/job/126045425871) 成功。macOS native job [126045426896](https://github.com/LordCasser/grow/actions/runs/41672827694/job/126045426896) 也已成功。Windows native 的 Shell coordination 组为 61 passed / 3 failed；三项 image-recovery 测试在模型能力状态 durable write 后收到 LocalResourcesStateStore 的 PublishedPersistenceError（OS 5）。日志见 `/tmp/grow-260-candidate-windows-ci-failure-excerpt.log`。该 store 强制目录同步；Shell 生产会话实际使用的 SessionResourcesStateStore 采用既有平台策略，不对 Windows 目录 handle 伪造 Unix fsync。独立 [fixture change](../2026-11-04-align-session-actor-resource-store-fixture/design.md) 将 Shell actor 测试接到实际会话 store，保留真实文件 flush/rename/ACK，不改变 Local store 的严格错误语义；该项后来在 Native 三平台成功的 run 41698917177 中通过。

在首轮 Core CI 失败后的阶段，candidate 尚未完成复验，也未发布。普通本地 release build、代码签名和散列记录当时不是官方 asset provenance；header fixture 彩色回归、required CI 与正式发布随后完成，详见下方历史候选记录和本文开头。

2026-11-04 用户明确确认本轮资源范围：修复进程树收尾，完成资源能力与隔离执行方案，不新增全局内存限额。该确认与已交付方案一致。

## 历史本机复验：首轮 CI 夹具修正

Header 固定主题与三个焦点状态的四项定向测试通过，彩色 Pager 全组 7566/0/15。Shell actor 改用真实 session resources store 后，standalone Shell 全组 4572 passed / 0 failed / 9 ignored（`/tmp/grow-260-session-resource-fixture-shell-final.log`），含 Windows 首轮失败的三个图片协调 selector；此前 combined core 的 Shell 4576 计数属于不同 feature unification，不能合并或替换。两项修正均为测试/注释维护，生产行为不变。静态 strict 19/19、归档 strict 837/837；当时新候选的 required CI 后来由 run 41698917177 等门禁完成。

## 历史 CI：第二轮候选

源码为 `f1ce5f35661ffe620855bb218656072b6e669ce4`。Header 修正已通过 Core 的 Pager library gate；Core [41679387890](https://github.com/LordCasser/grow/actions/runs/41679387890) 随后在 Shell 出生权限 cold-restore 用例失败：4572 passed / 1 failed / 9 ignored，child pool 为空而应保留 birth。`/tmp/grow-260-candidate2-core-ci-failure-excerpt.log` 保留详细记录。新发现是父 MCP 后台初始连接发布前冻结空 grant 的真实竞态，由 [独立行为 change](../2026-11-04-await-parent-mcp-before-cold-child-grant/design.md) 修复；不是缺出生事实，也不通过扩大权限解除。

Native [41679387744](https://github.com/LordCasser/grow/actions/runs/41679387744) 的 Linux/macOS jobs 已成功；Windows Shell coordination 64/64（含三个原失败 selector）、Pager coordination 47/47、Pager-minimal 4/4 均成功，确认 session-resource fixture 修正。Windows 随后编译 Tools lib tests 时失败：两个 Unix parser tests 引用 Windows 不编译的函数，见 [平台测试维护](../2026-11-04-gate-unix-only-terminal-tests/proposal.md)。整个 Native workflow 仍是失败。

## 历史本机准备：第三候选

父 MCP 初始化冻结竞态已由独立行为 change 合入规范。完整六种出生状态回归（含慢初始化、初始化拒绝）1/1 通过；最终调用方取消与即时命令响应性按真实 narrow suspend/restore 两阶段分别 1/1 通过。Shell 4572/0/9、MCP179/0、Unix parser2/2、background1/1、workspace all-target 与格式检查均通过。各阶段源码边界和完整日志见 [MCP 初始化修复验证](../2026-11-04-await-parent-mcp-before-cold-child-grant/verification.md)。只读对抗性复审核对了普通 Spawn 即时路径、取消/关闭的 None 返回、原 checkpoint 保留及 publication→state 锁顺序；没有发现当前范围的生产缺陷。父 actor 在该 waiter pending 时的关闭仅有源码验证，不冒称专项动态故障注入通过。

在第三候选阶段，Windows 的 Unix-only parser 编译修正仍待原生 CI；该修正随后由 Native 三平台 workflow 验证通过。正式 tag、preflight、十平台构建与发布也随后完成，见本文开头。

## 历史 CI 与 PTY 修订：第三候选

`fccd91b10e141d59d4ed87ca85629015a2949e55` 的本机完整 18-crate 套件为 19233 passed / 0 failed / 51 ignored（`/tmp/grow-260-candidate3-core18.log`，18 个 crate 最终 summary 汇总；`...-summary.json` 保存逐包计数）。这补齐了初始化修复与调用方取消之后的最终源码验证。Native run [41689811331](https://github.com/LordCasser/grow/actions/runs/41689811331) 三平台全绿；OpenSpec run [41689811476](https://github.com/LordCasser/grow/actions/runs/41689811476) 通过。Core run [41689811275](https://github.com/LordCasser/grow/actions/runs/41689811275) 的 libraries、Settings、CLI、三项真实协议恢复均通过，仅 Minimal Ctrl+D PTY 的观测/确认时序失败。

独立测试维护 [align-minimal-quit-pty-confirmation](../2026-11-04-align-minimal-quit-pty-confirmation/verification.md) 保留提示和退出断言，重新隔离一个一秒内的确认对。该阶段 CLI 对 Core 选中的全部 32 项 PTY 逐项执行，32/32 通过；`/tmp/grow-260-core-pty-results.json` 保存命令、状态、时长和日志。此修订不改变生产代码。当时精确提交的 CI、tag/preflight 与官方十平台资产仍待执行；后续结果见本文开头。

## 历史 CI：第四候选 455bcb（Core 阻塞）

候选提交为 `455bcb92475066e31a361c6c4f6160f33441ecdb`。Native run [41698917177](https://github.com/LordCasser/grow/actions/runs/41698917177) 的 macOS job [126136742123](https://github.com/LordCasser/grow/actions/runs/41698917177/job/126136742123)、Linux job [126136742520](https://github.com/LordCasser/grow/actions/runs/41698917177/job/126136742520)、Windows job [126136742531](https://github.com/LordCasser/grow/actions/runs/41698917177/job/126136742531) 均成功。OpenSpec run [41698917168](https://github.com/LordCasser/grow/actions/runs/41698917168) 成功。

Core run [41698917082](https://github.com/LordCasser/grow/actions/runs/41698917082) 失败于 PTY 用例 `suspend_nested_task_tree::ctrl_d_notification_turn_suspends_and_resumes_background_child`，断言 `272 !turn_has_ended`。此前 libraries、Settings、CLI、protocol 与 Minimal Ctrl+D gates 均通过；Core workflow 整体仍失败。根因在测试 fixture：gate 阻塞最后的 `[DONE]`，但 provider 已先发 `response.completed`，parser 在该 completed 事件处结束读取。独立 skip-specs 测试维护 change [hold-notification-pty-before-provider-terminal](../2026-11-04-hold-notification-pty-before-provider-terminal/verification.md) 移除两个被阻塞脚本中可选的 `[DONE]`，普通和 Minimal 的最终精确用例分别 1/1 通过；日志与源码边界见其验证记录。此修订不改生产代码；当时下一精确候选的 Core CI 尚待验证，之后 Core 和正式发布门禁均已通过。

这轮 CI 当时没有 dispatch 官方 release workflow。该候选的 Core 失败由后续独立测试夹具 change 修正；随后候选5475 CI 与 v2.6.0 正式发布均成功，具体结果见本文开头。首次官方 Core 超时的原因仍未证实。

## 发布记录归档收尾

`close-v2-6-0-release-blockers` 于 2026-11-04 在正式发布与独立资产核验后归档。活动 strict 16/16、归档 strict 844/844；归档链接按实际目标重新定位，原始 backlog 快照哈希不变。后续提交仅更新 OpenSpec 记录与链接；`v2.6.0` 仍固定在 `5475dff997f05cf6c67ab8ba24621ac8863c144a`，不把文档收尾提交当成发布二进制来源。
