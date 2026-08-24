# Changelog

# 1.1.11 — 2026-08-24

## Long-term Goal Behavior

- Goal now persists one objective, lifecycle status, budget, usage, and timestamps. Planner/verifier stages, Markdown boards, Goal-specific task types, and finalization turns were removed.
- Active Goals continue only after foreground and user FIFO are idle. Every continuation audits the full objective before creating the next ordinary task/todo slice.
- Goal v7 gives the user-controlled definition its own revision. Provider and compaction request assembly expand only the latest current continuation directive and shadow older copies without rewriting Timeline history.
- `/goal edit`, `pause`, `restart`, `budget`, and `clear` provide the user control plane; `create_goal`, `get_goal`, and `update_goal` are the complete model lifecycle API.
- Goal and Behavior transitions commit through one versioned Timeline Control snapshot and roll back on persistence failure.
- Pager Goal UI and Timeline updates now show durable lifecycle state only; streaming phase/progress noise is not persisted.

## Delegated authority

- Subagent capability resolution now has one default (`read-write`) and one parent-confinement rule: the requested/Agent mode is intersected with the immediate parent's immutable delegation ceiling before child resources are created. The former implicit `all` and spawn-rejection path were removed.

完整发布说明见 [changelogs/1.1.11.md](changelogs/1.1.11.md)。

# 1.1.5 — 2026-08-09

## Behavior Changes

- Behavior 在 turn 采纳时捕获进 `TurnContext`:该 turn 的 prompt、工具面与限制随后保持
  不变;切换 Behavior 不重标队列、不改变已运行的 turn。
- `BehaviorId` 成为 Shell、Tools 与 Pager 的唯一 Behavior 身份;Behavior、Plan、Goal、
  Deep Research 控制状态原子化持久化,`BehaviorCoordinator` 收敛为纯决策器。
- Goal 从整文档变更改为带修订版本的 Markdown board:planner 拥有任务结构,主 Agent 用
  `update_goal_progress` / `request_goal_replan` 类型化更新,不再整体替换 board。
- Goal 详情视图新增任务摘要与完整 board 查看器;Goal 生命周期以结构化 session events
  渲染,不再由 pager 猜测阶段。
- `read_file` 默认路由智能分类 PDF:文本层直接提取本地 Markdown,扫描/混合 PDF 保持
  渲染到视觉模型;新增 `format="markdown"`,`format="text"` 升级为阅读顺序提取,
  提取警告作为诊断行输出。

## Fixes

- 修复 plan 修订后 verifier 仍基于过期计划继续验证的问题(修订时自动取消 verifier)。
- 修复并发 turn 下的排队消息渲染与 turn 状态行不一致问题。
- `read_file` 移除全局 PDF 缓存,改用 anydoc 文档管线统一 pdf/pptx 处理。
- pager 动效与生命周期时钟解耦,完成事件驱动动效管线,spinner/wave 不再依赖 watchdog
  或 tick 分支推进。

完整发布说明见 [changelogs/1.1.5.md](changelogs/1.1.5.md)。

# 1.1.4 — 2026-08-05

## Behavior Changes

- Goal 验证、规划与总结改为后台 stage 执行：验证期间界面与输入完全可用，不再需要 clear 恢复；
  turn 终结收敛为单一权威，恢复链 watchdog 自动探测并恢复卡死的终结状态。
- verifier 判断完成期间，所有发送入口（普通消息、send-now、队列行发送、编辑保存、队列 drain）
  统一拦截并提示 `Goal is verifying; please wait before sending.`；消息保持排队，验证结束后
  自然发送。
- Goal Active 时 send-now 统一作为 mid-turn steering 注入（提升 autonomy generation），
  系统事件明确标记为非用户输入。
- Goal Active 时 Ctrl+C / Esc 打开 Goal 中断面板：Pause goal / Stop turn only /
  Stop turn + subagents 显式三选，无静默默认值；只有显式 "Pause goal" 才暂停
  Goal，其余取消保持 Goal Active 可继续；Esc 仅关闭面板。
- Goal round 作为普通 Running turn 展示与操作：round 期间 Enter 排队、send-now
  即 foreground steering。

## Fixes

- 修复 Goal 中断面板二次 Esc 取消父 turn 的问题；修复 goal stage 运行期间
  （session Idle）goal chip spinner / 已用时钟 / dashboard 行动画停摆的问题。

- 修复 Goal 执行中回复卡死、界面阻塞、/compact 等命令被卡住的问题。
- 修复 Goal 启动后反复生成 plan writer 的无限循环（校验宽容化、spawn 准入时机、planner
  单飞互斥、latch 泄漏清理）。
- 修复计划产物为空或发布失败时的静默重试；此时暂停 Goal 并说明原因。

完整发布说明见 [changelogs/1.1.4.md](changelogs/1.1.4.md)。

# 1.1.3 — 2026-08-03

## Behavior Changes

- 内部辅助模型请求遵循 BYOK 采样所有权：未显式配置时不再写死 `temperature`、`top_p`、
  `max_output_tokens` 或 `reasoning_effort`，由模型配置或上游服务决定。
- 模型目录不再自动选择最低 reasoning effort；Auto 分类器也不再默认注入 `low`。
- `read_file` 图片与渲染后的 PDF 页面可由 `[models].image_description` 指定的视觉模型解释；
  未配置时继续交给主模型直接读取。

## Fixes

- 修复只允许 `temperature = 1` 的模型在图片描述等内部调用中返回 HTTP 400。
- 扫描型 PDF 的多页图片会在一次辅助视觉请求中完整传递并生成文字工具结果。
- Auto 分类器可显式配置独立模型与 reasoning effort；省略 effort 时保持上游默认。

完整发布说明见 [changelogs/1.1.3.md](changelogs/1.1.3.md)。

# 1.1.2 — 2026-08-03

## Behavior Changes

- Goal 执行中接受普通排队输入；Send now 原子移入 mid-turn interjection，不取消当前 Goal。
- Goal 斜杠命令通过独立控制面执行并显示响应日志；`budget`、`resume`、`set` 不打断执行，只有
  `/goal pause` 会取消当前 turn。
- `/goal set` 原位修订目标并支持当前内容 Tab 补全；异步 Goal 角色结果受 definition revision
  约束，不能提交到已修改的目标。
- Goal 命令的状态提交与模型工作分离：Behavior 立即同步，planner 通过隐藏 turn 运行，避免 actor
  mailbox 在 `Planning` 状态自锁。Behavior picker 进入 Goal 后的首条文本等价于 `/goal set`。
- pause、budget 限制、目标修订和自动 back-off 保留 Goal Behavior；仅 clear 或 verifier 确认完成
  自动回到 Normal，暂停/取消边界上尚未消费的用户 steering 会保留到队列。
- `read_file` 图片与渲染后的 PDF 页面按 `[models].image_description` 显式路由：未配置时交给
  主模型，配置后由指定辅助模型描述且失败不回退。

完整发布说明见 [changelogs/1.1.2.md](changelogs/1.1.2.md)。

# 1.1.1 — 2026-08-02

## Breaking Changes

- **认证正式收敛为 BYOK-only。** Provider 凭据只能来自 `api_key`、`env_key` 或本地密钥
  helper；遗留 OAuth/OIDC、设备登录和 refresh-token 生命周期已删除。
- **Session 只保留 Grow 本地语义。** 外部产品 session 扫描、vendor tool mode 和相关兼容
  恢复路径已删除；已有 Grow session 仍按保存的 Agent、模型和 reasoning effort 恢复。
- **Agent 定义不再承载运行时状态。** Provider、model、Behavior 和 Permission 属于 session；
  Agent Markdown 使用严格 frontmatter，未知或越界字段会直接报错。

## Features

- **权限提示强制超时。** 交互 session 默认 60 秒，非交互 session 默认 10 秒；超时以
  `permission_timed_out` 取消当前 turn，工具不执行，迟到授权不会持久化。两个期限可在
  `[session]` 配置，但不能设为 `0`。
- **Agent、Behavior 与 Permission 完成解耦。** Role 只定义职责和工具范围，Behavior 只定义
  当前任务的推进协议，Permission 独立决定工具授权；TUI selector 与 slash command 共用同一
  状态源。
- **模型与恢复更可预测。** 模型目录完全由用户 Provider 配置生成；已有 session 恢复保存的
  Agent/模型/effort，新 session 使用全局默认值。
- **Release 扩展为九个目标。** 官方分发覆盖 macOS、GNU/musl Linux 与 Windows 的
  x86_64/arm64，并额外提供 Linux riscv64；九个最终资产齐全后才公开 Release。

## Reliability

- 截断响应可自动续写，context overflow 可按预算压缩并恢复原请求。
- 会话关闭会回收 bash、后台命令、Hook、LSP、stdio MCP、PTY 和子 Agent 的进程树。
- 权限 pending interaction 在超时、取消或应答后统一关闭，leader 重连不会重放已经解决的弹窗。
- BYOK helper 返回裸 JWT 时会读取 `exp` 作为刷新期限，并保持 `expires_in`、配置 TTL、JWT
  claim 的明确优先级，避免近过期 token 被长期缓存。
- Session 搜索缓存损坏可自愈，启动线程耗尽会降级，crash handler 增加 SIGABRT 捕获。

## Maintenance

- 第一方 crate 按实际复用和所有权重新收敛，删除无消费者的产品 service、scanner、协议和依赖。
- Release helper、tag/version 校验、updater 平台选择与九目标 matrix workflow 对齐。

# 1.0.0+upstream — 2026-08-01

## Upstream Sync (grok-build 8d69c91f, curated)

策展合入上游 2026-07-28 ~ 07-31 的 4 个同步提交中与本项目架构兼容的部分。

## Features

- **进程回收**：会话关闭时回收子进程树（bash/后台命令、hook、LSP、stdio MCP、PTY 全树、agent 子进程与 idle inhibitor）；子代理继承根会话的进程作用域；PTY shell 注册前自回收。
- **稳健性**：crash handler 捕获 SIGABRT；启动线程耗尽降级而非崩溃；tokio worker 线程上限（GROW_WORKER_THREADS）；会话搜索 SQLite 缓存损坏自愈；fast-worktree resume 不再误删用户注册；doom-loop 恢复默认开启。
- **采样与会话**：messages 后端缓存增长中的 transcript；truncation continue 与 context-overflow 恢复（fit 阶梯）；"exceeds budget" 归类为 context overflow；auth 401 归属（sent credential 溯源）；会话关闭取消子代理与进行中压缩（[stop]）；/btw 模型过载自动重试；删除当前会话（/delete）。
- **Pager**：Kitty 键盘协议按终端版本降级；DA2 终端版本探测；minimal 模式 plan 驻留 scrollback；全屏 resize 提速；settings 枚举选择器保持已提交值；dashboard/welcome 删除会话；Ctrl+. 快捷键表补充 history/search；壁钟计时（OS 挂起不再计入工作时长）。
- **工具**：工具描述/输出矛盾修复；blocking-wait 上限可配置（GROW_MAX_WAIT_BLOCK_MS）；LSP 拉取式 C# 诊断；MCP 服务器 CLI enable/disable；.grow/sandbox.toml 保护编辑；git_commit 硬化与 git_sync_base。
- **其他**：外部 auth 提供命令走平台 shell（修 Windows）；前端 frontmatter 颜色宽松解析；/undo 别名；session 持久化 actor 随会话退出；后台任务/子代理跨压缩保留；MCP OAuth 有界锁与新鲜度守卫。

## Removed Product Features (still excluded)

- 本批次上游提交中的 xAI 产品内容（SuperGrok 订阅、Coding Data Sharing、遥测、托管 Web Search、远程会话、voice、share 链接、chat-kind、本地 workspace 特性、团队托管配置、extra CA bundle）均未合入。

# 1.0.0 — 2026-07-30

## Breaking Changes

- **Grow is provider-neutral and BYOK-first.** No Grok model, inference endpoint, credential, billing path, or xAI product service is built in.
- **Configuration uses explicit providers.** Models live under `[provider.<id>.models]`; new sessions use `[models].default`, while restored sessions keep their last model and reasoning effort.
- **Distribution is GitHub Release only.** The supported targets are macOS arm64, Linux arm64, and Linux amd64.

## Features

- **Multi-provider model selection** supports Chat Completions, Responses, and Messages backends, provider-scoped API keys or OAuth, model-specific reasoning efforts, and backend-neutral `output_limit`.
- **Agent selection** loads user Agents and Skills from `~/.grow` only (project `.grow` + user `~/.grow`) and keeps Agent definitions independent from provider and model selection.
- **Local announcements and diagnostics** retain the existing TUI surfaces without remote delivery or telemetry exporters.
- **Replaceable integrations** allow users to configure marketplace sources and MCP servers, including MCP-provided Web Search.

## Removed Product Features

- Telemetry upload, remote trace storage, billing, credits, subscriptions, upsells, hosted Web Search, remote conversations, remote announcements, computer-hub remote connections, voice, and image/video generation.
- xAI-bound plugin marketplace registration, product announcements, and Grok-specific protocol identifiers.

## Build and Release

- All `grow-*` workspace crates now inherit version `1.0.0` from the workspace package metadata.
- Default Agent prompts are compiled directly from readable Markdown under `agent/prompts`; the legacy prompt encryption and generated Rust wrapper are gone.
- Release builds embed a verified ripgrep binary and are published with checksums by the GitHub release workflow.

# 0.2.112 — 2026-07-24

## Breaking Changes

- **CLI version policy** now has separate soft update floors/ceilings and hard startup requirements.

## Features

- **New /tutorial slash command** opens an opt-in nine-topic onboarding tour of Grok.
- **New config options** let you add query parameters or environment-backed headers to custom model providers and control which variables reach shell tools.
- **Terminal and environment fixes** are now consolidated under the `/doctor` command with clearer guidance.
- **Marketplace add** now rejects non-git URLs at add time instead of failing later.
- **Slash commands** can now show optional bracket tags (e.g. [new]) via config or remote settings.
- **Queued prompts** now offer an [edit] mouse button alongside Send now and cancel.
- **Voice shortcut** toggle in settings can disable the Ctrl+Space/F8 keybind without disabling voice entirely.
- **Image edit** can now use a remotely configured model slug instead of the hardcoded default.
- **`grok doctor fix`** can now repair common tmux clipboard and passthrough problems.
- **Per-provider auth helpers** now work on Windows and can run from a configurable working directory.
- **/resume** now shows only native Grok sessions by default and shows a hint when external sessions are hidden.
- **`grok --resume`** can now resume a session by its title as well as by ID.
- **Workflows overlay** now shows live per-agent progress and automatically follows the active phase.
- **Workflow runs** that failed can now be resumed; scratch file limits were also increased.
- **Hooks** can now be defined in config.toml in addition to JSON files.
- **Clicking** the "still running" status now opens the tasks pane.

## Bug Fixes

- **File attachments** now appear correctly when resuming or replaying conversations.
- **Terminal output** from remote clients is now recorded so read-file hints and monitors function correctly.
- **Background shell commands** now correctly report their real exit codes instead of always showing -1.
- **Marketplace source refreshes** no longer hang the TUI or trap you in the extensions modal.
- **Background task tray** now correctly clears killed tasks and keeps task descriptions after reconnect.
- **Dashboard overlay** now correctly returns after forking a dashboard-attached session.
- **Linux voice dictation** now works on PipeWire versions before 1.6.
- **Fork** from a rewound session now copies the correct live-branch history.
- **Account pane** now shows name and email even after the access token expires.
- **Voice mode** now lets you edit already-dictated text without closing the microphone.
- **Fixed startup hangs** on Linux after concurrent launches or rapid restarts.
- **MCP tools** now appear without restart after enrolling or updating a managed service.
- **Plugin subagents** now see the same MCP tools as the parent session.
- **Copy confirmations** now show shorter messages when the clipboard succeeds.
- **Repeated identical tool calls** now end the turn silently instead of showing a stop banner.
- **Web search** now defaults to grok-4.5.
- **Voice dictation** text is no longer dropped when pressing Enter to send.
- **Bash mode** (`!`) now shows yellow prefix and action label in minimal mode.
- **Parked turns** no longer spam duplicate "Worked for" markers in the transcript.


# 0.2.111 — 2026-07-22

## Features

- Users can now disable image generation and video generation tools (and their slash commands) via config.toml or environment variables.
- `/session-info` now displays whether the session uses OAuth or an API key and where to manage the account.
- You can now run `grok doctor fix` commands directly from inside the TUI instead of only from the CLI.

## Bug Fixes

- **Plugin subagents** now inherit the parent session’s connected MCP servers (default `mcpInheritance: all`), so `search_tool` / `use_tool` work the same as for local agents. Plugin agents still cannot declare their own MCP servers, hooks, or elevated permission modes.
- **`!cmd` commands** now allow up to one hour before timing out.
- **npm package** now installs the native binary under `$GROK_HOME/bin` (honoring the same override as the Rust CLI).
- **Startup warnings** now point to `/doctor` for details and fixes.
- **Dashboard hover and clicks** no longer miss the gaps between items in wide mode.
- **Shift/Alt+Enter** now inserts a newline while editing a queued prompt.
- **Queued prompt edits** under combine mode no longer lose changes due to premature hold release.
- Forking a session that used compaction no longer causes later rewinds to fail with missing checkpoint errors.
- When a permission prompt appears while viewing scrollback, focus now correctly moves to the prompt so you can answer.
- Pressing Esc once now cancels the current agent turn (except in fullscreen vim scrollback mode).
- Grok now automatically stops a turn that keeps repeating the exact same tool call many times in a row.
- Configs using either spelling of the workspace teleport disable flag now load and save correctly.
- Background subagent completion messages no longer leak into unrelated sessions when multiple sessions are active.
- When the auto-permission classifier times out or fails, Grok now shows a normal permission prompt instead of silently denying.
- **Managed MCP tools** no longer time out prematurely on slow operations like Notion updates.

## Performance

- Voice dictation on macOS now uses less memory by running capture in a temporary helper process.


# 0.2.110 — 2026-07-21

## Features

- **Removing MCP servers, plugins, or hook sources** in the Extensions modal now asks for confirmation (press y to proceed).

## Bug Fixes

- **Session creation failures** (including disk full) now show an error message instead of hanging on "Starting session…".
- **Auto-compact** that fails due to an expired token now lets you log in and automatically retry the compact + original prompt.


# 0.2.109 — 2026-07-21

## Features

- **/usage** now shows token counts and cost for the current session.
- **grok doctor fix ssh-wrap** can set up `grok wrap ssh` automatically for Bash, zsh, and fish.
- **[model_providers.<id>]** lets operators share gateway settings across custom models.
- **Reasoning effort** now accepts `max` as its own tier (above `xhigh`) when the model advertises it.
- **Queued follow-ups** can now be batched into a single model turn with the new combine_queued_prompts setting.
- **/doctor** is now the main in-app command for checking terminal, tmux, clipboard, and keyboard setup.
- **read_file** now returns full Markdown files inside skills/ directories without truncation.

## Bug Fixes

- **Voice dictation** now explains when the microphone delivered only silence (macOS permission) versus no speech detected.
- **Duplicate 'Worked for' markers** no longer stack in the transcript when background tasks defer during a parked turn.
- The idle status row now clearly says '1 subagent still running' instead of 'watching · 1 subagent' when background work remains.
- **Background /loop** iterations no longer overlap when descendant subagents are still running.


# 0.2.108 — 2026-07-21

## Features

- **Sessions** can now be resumed after moving the working directory or switching machines.
- **Ctrl+G** in minimal mode opens the current prompt draft in an external editor without sending it; fullscreen keeps the tasks pane.
- **grok doctor** checks terminal, tmux, clipboard, and keyboard setup without opening the TUI.

## Bug Fixes

- **Image paste** over grok wrap now works on headless remotes.

# 0.2.107 — 2026-07-20

## Features

- **Stop hooks** can now keep the agent running by feeding feedback back to the model instead of ending the turn.
- **Custom models** can now authenticate using rotating tokens fetched from a command, similar to credential helpers.
- **Feedback** now includes author details when provided, helping with follow-up.
- **Sessions** can now resume across hosts by mirroring transcripts to external storage like S3.
- **Sessions** can now be imported and resumed from mirrored state across hosts.
- **Auto mode** now continues after classifier blocks by telling the agent the reason, escalating only after repeated denials.
- **Session storage** can now flush after every frame (eager mode) instead of only at turn end.

## Bug Fixes

- **Ctrl+B** now backgrounds running commands; **Ctrl+G** toggles the tasks pane.
- **OAuth popups** in live preview now redirect correctly after login.
- **Git status** shown to the model at startup now includes unstaged and untracked files.
- **Tool descriptions** now stay correct when parameter names are randomized.
- **Minimal mode** now shows full reasoning in scrollback and collapses successful lookup results to one-line headers.
- **Empty commands** like `true` or bare `echo` now remind the model to stop and wait for background work instead of spinning.

## Performance

- **Recap summaries** after idle now load much faster by reusing the previous turn's cached context.


# 0.2.106 — 2026-07-18

## Features

- **Added GROK_CLIPBOARD_NO_OSC52** env var to stop clipboard sequences from appearing as garbage in unsupported terminals.
- **Scheduled tasks** can now be updated in place; one-time tasks are retired in favor of background commands.

## Bug Fixes

- **Copies** now always write a backup file so text remains recoverable when the terminal clipboard fails.
- **Syntax highlighting** in --minimal mode is now visible on light terminals.


# 0.2.105 — 2026-07-18

## Features

- **/btw** now works inside `grok --minimal`, showing answers in the live area and committing them to scrollback on Esc.
- **New Appearance setting** "Snap prompt to top on send" lets you keep the viewport where it is instead of jumping to the new prompt.
- **Default model** is now Grok 4.5 with high/medium/low reasoning effort and improved compaction settings.
- **New `/summarize` slash command** is now available as an alias for `/recap` to request an on-demand session summary.

## Bug Fixes

- **Local shell tools** now see the same environment variables, aliases, and functions as your login shell.
- **Syntax highlighting** in diffs and the file viewer no longer miscolors strings or comments that span multiple lines.
- **Global rules** from ~/.grok/rules and compatible vendor homes are now discovered correctly.
- **Background tasks** that finish after you press Ctrl+C no longer automatically resume the model.
- **Ctrl+\** out of the dashboard now returns you to the agent you came from.
- **MCP OAuth logins** now succeed against servers that require the RFC 9207 issuer parameter in the callback.
- **Agent dashboard** now shows fleet roster entries even when the local agent list is empty.
- **Long-session compaction** no longer fails on servers that reject tool_choice none when tools are attached.

## Performance

- **Scrolling** feels smoother and less jagged under load or over slow connections.


# 0.2.104 — 2026-07-17

## Features

- **Background work counts** now appear in a persistent status line instead of repeated transcript messages.

## Bug Fixes

- **Fixed authentication recovery** for idle sessions after token timeouts.
- **Retry failed** messages no longer contain raw HTML error pages.
- **Rate limit messages** now show the server detail without the wire prefix.
- **In-place prompt editing** is temporarily disabled due to scroll behavior issues.


# 0.2.103 — 2026-07-17

## Features

- **New require_sha option** prevents remote plugins from tracking mutable branches or tags.
- **Local sessions now inherit full rc environment, cwd, and exports** across tool calls (configurable).
- **MCP servers** from plugins can now require setup choices such as a regional site before connecting.
- Quitting a fullscreen session now shows the session title and last exchange above the resume command.
- **SSH sessions** now show a one-time tip recommending `grok wrap ssh <host>` for clipboard and terminal restore.

## Bug Fixes

- **Fixed GitHub PR status detection** when the gh CLI inherits forcing color environment variables.
- **Fixed a race** where an early cancel could permanently wedge a session's turn slot.
- **grok** and the agent binary now stay in sync even when no update is installed.
- **Copying** a multiline queued prompt now copies the complete text instead of a collapsed summary.
- **grok wrap** now restores the terminal after SSH disconnects or other abrupt child exits.
- **Voice speech-to-text** now works with per-model API keys in config.toml without requiring `grok login`.
- **Copy over SSH** or in containers now shows clearer feedback when delivery cannot be confirmed.
- **Local Bash sessions** no longer keep a persistent shell across calls, avoiding failures after directory deletion.


# 0.2.102 — 2026-07-16

## Breaking Changes

- **--minimal** and **--fullscreen** flags now apply only to the current session.

## Features

- **New /jump slash command** lets you quickly jump to any previous turn in the conversation.
- **New /timeline sidebar** shows a clickable tick rail for fast navigation between conversation turns.
- **grok login** now requests Grok Projects scopes so workspace listing works after consent.
- **Permission mode** can now be set fleet-wide via remote config when no local setting exists.
- **Edit tool output** has a setting to show a compact one-line summary instead of always-expanded diffs.
- **Tab completion** in !bash mode now works like a normal terminal (prefix fill, dropdown, directory drill-down).
- **Enterprise deployments** can now disable voice dictation via `requirements.toml` so `/voice` and Ctrl+Space are hidden for everyone.
- **User prompts** now appear bold only in `--minimal` mode; fullscreen keeps normal weight.
- **`grok plugin install`** now accepts a marketplace's registered name as a qualifier.
- Consecutive edits to the same file now collapse into a single scrollback row when collapsed edit blocks are enabled.
- Local sessions now inherit your shell environment variables and keep the current directory across commands.

## Bug Fixes

- **Login and re-login** no longer stack multiple device-code polls or leave stale flows running.
- **Background task tools** now render with correct icons and titles instead of the generic MCP wrench.
- **Task tool** now correctly validates and displays allowed model slugs for subagents.
- **Rewind** now correctly handles bash transcripts, permission follow-ups, and sessions that mix old and new prompt markers.
- **Re-login** during a session now immediately uses the new token instead of requiring a new session.
- **Terminal commands** using globs now behave the same on zsh as on bash and no longer fail with shell errors.
- **Installer** no longer replaces stowed shell configuration symlinks with plain files on upgrade.
- **Voice transcription** now works with enterprise API bases and API-key authentication.
- **Fixed crashes** on some network-mounted home directories by using a safer SQLite journal mode.
- **Home and End keys** now move to the ends of the current wrapped line in the prompt.
- **Arrow keys and Esc** now work correctly inside viewers opened from the dashboard.
- **Warns at startup** when user and project sandbox profiles define the same name differently.
- **Billing upgrade links** now show the full URL in the transcript (and copy it) when a browser cannot be opened.
- **Fixed Ctrl+Y yank** no longer working after sending a prompt.
- **No longer shows permission prompts** seconds after a turn was cancelled with Esc or Ctrl+C.
- **Page Up and Page Down** now move the highlighted entry to the top or bottom of the visible scrollback area.
- Conflicting project and user sandbox profiles now show a clear warning on the welcome screen.
- **OAuth login URLs** no longer contain duplicate referrer parameters.
- **File links** in official VS Code Remote-SSH terminals now use VS Code's native path handling.
- **Minimal mode** now shows the folder-trust prompt after sign-in when required.
- **Skills** whose names collide with built-in slash commands are now reachable via qualified names.
- **Fixed background task tracking** when using grok -p --no-wait-for-background so tasks are properly reaped on exit.
- **Rate limit errors (429)** now show specific server messages (capacity, team limits, free-usage) instead of generic upgrade prompts, with correct copy based on auth type.
- **`/copy` slash command** is now available in minimal mode.

## Performance

- **Improved recap and compaction** behavior.

# 0.2.101 — 2026-07-13

## Features

- **grok inspect** now shows effective compatibility settings for Cursor, Claude, and Codex sessions.
- **New setting** "Match display refresh rate" lets high-refresh displays run the TUI at native cadence.

## Bug Fixes

- **Parked subagent status** no longer duplicates or interleaves incorrectly in scrollback.
- **Status line** during waits now shows elapsed time before the queued-message hint.
- **Queued messages sent with Enter** now appear immediately instead of vanishing briefly.
- **Resume hint** after quitting minimal mode now prints the correct grok --minimal --resume command.
- **Rate-limit messages** now correctly direct API-key users to team plans instead of personal upgrades.


# 0.2.100 — 2026-07-13

## Features

- **Session picker** now discovers and resumes recent Claude Code, Codex, and Cursor sessions.
- **Welcome screen** now offers a one-click resume nudge for recent Claude, Codex, or Cursor sessions.

## Bug Fixes

- **Web fetch tool** preserves full truncated page content as readable artifacts instead of discarding it.
- **Multiline mode** now correctly sends the top queued message on empty Enter when a turn is running.
- **Queued commands** no longer disappear or delay when pressing Enter twice quickly during a running turn.
- **Minimal mode** text is now readable on dark terminals with proper contrast and highlighted user prompts.
- **Grok no longer crashes** when printing resume hints after the terminal pane has closed.
- **Long-running turns** with multiple waits now show updated status markers in the transcript instead of appearing stuck.
- **Claude and Cursor hooks** are now correctly disabled at session start when disabled in config.


# 0.2.99 — 2026-07-12

## Features

- **Multiline input** now works on the agent dashboard the same way it does in regular sessions.
- **PageUp and PageDown** now scroll the conversation while the prompt is focused.
- **Keyboard Shortcuts** modal now follows Vim mode navigation keys when enabled.


# 0.2.98 — 2026-07-12

## Breaking Changes

- You can now pin authentication to API key or OIDC in config.toml; the unpinned method is no longer tried automatically.

## Features

- **`/context`** now shows token costs for skills and MCP servers.
- **`env_key`** in config now accepts an array of environment variable names.
- Linux middle-click paste from the primary selection now works; clipboard errors are handled more reliably.
- **/terminal-setup** now shows your terminal's color support level and which themes are available.
- **grok setup --json** prints your team's managed configuration without installing it.
- Messages you type while the model waits on tasks now stay queued; pressing Enter twice sends them immediately by cancelling the current turn.
- **How-to Guides** modal now shows a tip linking to Ask Grok above the footer shortcuts.
- **Subagent** `task` and `spawn_subagent` tools now accept an optional `model` parameter in the CLI.
- **Keyboard Shortcuts** modal now lists the paste key binding for images under the Input section.

## Bug Fixes

- A `pre_tool_use` deny now feeds the reason back so the model can retry instead of cancelling the turn.
- Plan mode now strictly rejects edits outside the plan file, even under always-approve.
- **Web search** and X search no longer fail when both a local function tool and the backend hosted tool are active.
- **Content-filter refusals** from providers now show an explanation instead of ending silently with no output.
- **SQLite databases** no longer cause bus errors on network filesystems such as NFS.
- **Resuming** a session that is already open now focuses the existing view instead of creating duplicates.
- **Turn completion** markers in scrollback now read "Worked for …" instead of "Turn completed in …".
- **/btw** loading spinner now animates correctly when the main session is idle.
- **Mid-turn** wait spinners now correctly show "Waiting on task output…" instead of Thinking.
- **Scrollbar thumb** is now visible in the oscura-midnight theme.
- Status messages for background work now end with a period.
- **Editing queued prompts** no longer freezes the terminal or duplicates text into the composer.


# 0.2.97 — 2026-07-11

## Features

- **Headless JSON output** now includes token usage and cost per prompt and session.
- **SDK turns** now expose detailed token usage and cost information via Turn.usage.
- **Double-click or Enter** on a previous user message now lets you edit and resubmit it directly from the transcript.
- **Text selection** in scrollback now works better when starting on chrome, gaps, or while scrolling.
- **Shell commands using `rg`** (ripgrep) no longer require permission prompts by default.
- **Voice mode** is now available for API-key sessions.
- **New environment variables** allow tuning scroll and draw cadence for high-refresh displays.

## Bug Fixes

- **Background tasks** started by the model in headless mode are now killed on exit instead of leaking.
- **Agent process leaks** on failed spawns and missing-stdio teardown are now prevented.
- **Parked turn markers** no longer appear after interjections and now count down as background tasks finish.
- **The /context** tool definitions line no longer shows the cryptic disclaimer suffix.
- **Terminal release** waits boundedly for the process to exit and repeated waits share the drain grace.
- **Fixed a crash** when the agent asked a question on narrow terminals.
- **Fixed misleading keyboard hints** in the /mcps panel in minimal mode.
- **Clipboard copy** now reports success correctly when using iTerm2 over SSH.
- **Fixed scroll/input conflicts** when plan approval appeared over an open edit block.
- **Fixed frequent MCP and skills reloads** that could freeze sessions on devboxes.
- **MCP servers using HTTP** (such as HTTP MCP servers for Slack) now automatically recover from disconnects.
- **Next reset time** in /usage now shows in your local timezone instead of Pacific Time.


# 0.2.96 — 2026-07-10

## Features

- **System notifications** now carry structured kind/title/body for better rendering.
- **grow/pr/status** now reports whether an open PR is in the merge queue.
- **Compact mode** now activates automatically on very small terminals.
- **Up arrow** on an empty prompt now browses prompt history; `/history` searches it.
- **Stop hook runs** now appear inline on the turn-completed line instead of a separate block.
- **Subagent rows** now fold into verb-group headers and the tasks pane shows live activity labels.
- **Dashboard shortcuts** now advertise ? instead of Ctrl+. on terminals that cannot deliver the latter.
- **Double-clicking** scrollback while Text selection is fold/nav now shows a tip offering Ctrl+Y to enable Word select.
- **`grok worktree ls`** now works as a short alias for `grok worktree list`.
- **MCP tool output truncation** can now be set per-repo in `.grok/config.toml`.
- **Auto-send of queued follow-ups** during task waits can now be enabled fleet-wide via remote settings.
- **Welcome screen** now offers one-click resume of a recent Claude Code session via ctrl+u.

## Bug Fixes

- **Vim `l` key** now opens the selected agent detail view in the dashboard.
- **Terminal commands** with no args now run through a shell, matching the CLI.
- **Agent teardown** no longer crashes on slim Linux images that lack the ps command.
- **Esc** now dismisses an open /btw panel before backing out of a dashboard overlay.
- **Resumed grok.com chats** now use the conversation's last model instead of the gateway default.
- **JetBrains terminals on Windows** now default to minimal mode to avoid raw mouse-report leaks in the prompt.
- **Skill token highlights** now survive line wraps and the slash menu opens when typing / before existing text.
- **Truncated or tiny images** are now dropped before sending and previously poisoned sessions self-heal on restart.
- **Session switch hints** after `/new` or fork now show the working command in minimal mode.
- **Progress bars** in Ghostty and WezTerm now stop correctly for parked task waits.
- **`/effort`** now rejects levels the current model does not support instead of sending a bad value to the API.
- **`/recap`** on a fresh session now says "No messages yet" instead of failing.
- **Monitor and system messages** no longer appear as user prompts when resuming old sessions.
- **`/rewind`** completion now appears as a brief toast instead of a permanent transcript line.
- **Auto recaps** no longer appear under a newer user message when you start typing again.
- **Authentication retries** after token refresh no longer hang for minutes or days.
- **Text selection tip** no longer appears on the first double-click or on non-assistant blocks.
- **Skill slash commands** queued while a turn runs can now be sent immediately with Enter or the interject chord.
- **Drag text selection** now works inside the dashboard dispatch input box.
- **Multi-line paste chips** in dashboard inputs now support preview and expand like the main prompt.
- **Live previews** now always fetch the latest content without browser or CDN caching.


# 0.2.95 — 2026-07-09

## Features

- **Teams** can now ship default allowed commands via managed_config.toml (user deny rules still win).
- **Mid-turn interjections** now appear as normal user prompts (❯) instead of a separate cyan block.

## Bug Fixes

- **IME text input in Otty** no longer attaches unrelated clipboard images on every character.
- **Rewind** now fully removes the selected turn from both scrollback and the model's conversation history.
- **Queued prompts** now abort long blocking waits instead of waiting for the full timeout.
- **File links and media** now work for worktree sessions under ~/.grok/worktrees/.
- **Collapsed Read/Edit tool rows** now show only the filename instead of long absolute paths.
- **Clipboard copies on Wayland** now succeed even when the terminal loses focus mid-copy.
- **User messages queued** behind an auto-wake turn are no longer lost when the user presses Ctrl+C.
- **Slash completion** now shows sibling skills that share a frontmatter name and correctly sizes wrapped descriptions.
- **Single tool calls** that belong to a verb group now collapse into an aggregated header row.
- **Fixed sessions** that became permanently stuck after tool-use history corruption.
- **/always-approve** and **/auto** now toggle their mode on and off when run repeatedly.
- **Terminal command cards** on grok.com now correctly settle after foreground bash tasks.
- **Copy failure** toast now recommends trying /minimal for native terminal rendering.

## Performance

- **File watching** on Linux now uses far fewer system resources for large projects with many dependencies.


# 0.2.94 — 2026-07-09

## Features

- **/sessions** now opens the Agent Dashboard instead of a separate picker.
- **New /goal <objective>** slash command** is now available when the workspace supports it.
- **grok inspect** now lists skills from [skills].paths and correctly labels bundled vs user skills.
- **--minimal** and **--fullscreen** choices are now remembered for future plain grok launches.

## Bug Fixes

- **Queued bash commands** promoted at turn end now render their output instead of disappearing.
- **Xcode / Foundation ACP clients** can now drive grok agent stdio without silent parse drops on session/* calls.
- **read_file** now returns full single-line content (minified JSON, large dumps) instead of silently clipping at 2000 characters.
- **Background task** command preambles with newlines now render on separate lines instead of collapsing.
- **Text selections** now highlight uniformly even over inline code, links, and syntax-colored spans.
- **grok --minimal** now supports native drag-select on classic Windows conhost terminals.
- Skill tokens such as /pr-workflow are now highlighted teal when used mid-sentence.
- Fixed a crash when a filtered list shrinks while the filter is active.
- Scroll lines and scroll speed settings now support fine unit-step adjustments.
- Project-specific Claude plugins are no longer visible outside their project directory.
- First prompt no longer stalls for many seconds on large repositories while the filesystem watcher starts.


# 0.2.93 — 2026-07-08

## Breaking Changes

- **Esc** no longer cancels a running turn; use **Ctrl+C** instead. Double-Esc rewind now works while focused on scrollback.

## Features

- MCP permission prompts now show the planned arguments so you can judge what the tool will actually do.
- The "Managed by grok.com" link in the Extensions modal is now clickable and underlined.
- Dragging inside rendered markdown tables now selects whole cells or rectangular ranges and copies as TSV.
- Shift+Tab now goes straight to Plan mode when the plan-mode tip is showing.

## Bug Fixes

- **grok --minimal** now aligns the prompt, status bar, and messages flush-left with the welcome card.
- **/plugins** no longer lists never-installed Claude marketplace entries and now groups plugins by their real source.
- Successful image compression no longer leaves a permanent line in the transcript.
- **--no-ask-user** now also disables ask_user_question for subagents.
- **--no-ask-user** now also disables ask_user_question for subagents.
- **Fixed a crash** shortly after launch on some systems caused by the diagnostics exporter.


# 0.2.92 — 2026-07-08

## Features

- **/minimal** and **/fullscreen** commands let you switch the current session between minimal and fullscreen modes.
- **ask_user_question tool** can now be enabled or disabled via config.toml, environment variables, or remote flags while defaulting to on.

## Bug Fixes

- **User-run shell commands** now display their complete output after finishing instead of silently dropping middle lines.
- **Edit tool output** now correctly highlights multi-line strings and scopes that previously spilled across hunks.
- **Always allow** grants for MCP, web_fetch and bash now take effect immediately in auto mode without re-prompting.
- **Cmd/Ctrl+click** on bare http(s) links now opens only once on Warp terminals.
- **Cmd/Ctrl+click** now works on imagine media paths and URLs that wrap across multiple terminal rows.
- **grok update** on Windows no longer fails when a previous .old executable is still running.

## Performance

- **Pasting images** on macOS is now ~65× faster by reading the pasteboard directly instead of via osascript.


# 0.2.91 — 2026-07-07

## Bug Fixes

- **Voice dictation indicator** and stop button now remain visible and clickable during plan mode review.
- **New Worktree dialog** now expands to show long names and scrolls with a leading … when the terminal is narrow.

# 0.2.90 — 2026-07-07

## Features

- **New /minimal and /fullscreen slash commands** let you switch the current session between minimal and fullscreen modes without quitting.
- **Session titles** from /rename now appear on the prompt box border after resume.
- **grok models** banner now correctly reports per-model API keys and deployment keys.
- MCP tool output size limit is now configurable via environment variable, config.toml, or remote settings (default unchanged).
- You can now add a local directory as a plugin marketplace source with `grok plugin marketplace add`.
- **Auto permission mode** now prompts far less often on routine development commands.
- Short media paths the model prints (images/1.jpg) are now clickable and open the file.
- **Preview** now prefers common dev ports like 8080 when multiple HTTP servers are detected.

## Bug Fixes

- **Model list now refreshes** after upgrading from a free to a paid subscription tier.
- **Extensions modal** now shows clearer enable/disable and install hints that match what each key actually does.
- **Folder trust** no longer prompts for or scans the entire home directory when it is a git repo.
- **Code blocks** no longer lose their background shading on the final line of unterminated fences.
- **Plan mode** now activates immediately when toggled during an active turn instead of waiting for the next prompt.
- Clicking back into the terminal window now focuses the prompt immediately when a permission or plan-approval panel is waiting.
- Paths the model emits inside quotes no longer end with literal backslash-n characters and cause file-not-found errors.
- **Next reset** time shown by /usage is now correct during daylight saving time.
- The ask-user-question tool now waits up to 30 minutes by default before timing out.
- The free-usage paywall no longer offers a Try Again button.
- Inline images no longer bleed through when entering or leaving the fullscreen subagent view.
- The [Open Image] button under generated media is now colored like a link.
- **Preview routing** now auto-selects well-known ports like 8080 even with framework signals on obscure ports.
- **Login screen** now centers the authentication URL when it fits on one line.
- **Enter key** now queues follow-ups by default while the agent waits on task output.


# 0.2.89 — 2026-07-07

## Features

- **Voice dictation** now works on Linux (requires pipewire, pulseaudio-utils or alsa-utils).
- **New /auto slash command** switches to classifier permission mode; the menu now shows only the other mode.
- **--effort** and **--reasoning-effort** are now interchangeable CLI flags for setting reasoning effort.
- **Image edits** now use the higher-quality Imagine model for better output.

## Bug Fixes

- **Try Again** on the free-usage paywall now correctly resubmits after rate-limit retries.
- **Cursor** now respects your terminal's default blink style instead of always blinking.
- **Skill commands** in scrollback now highlight only the command name, not the arguments.
- **Plan files** now default to .grok/plan.md to match Grok conventions.
- **LaTeX math** renders correctly for display equations and complex subscripts.
- **Queue hint** in the terminal no longer shows incorrect bold text on part of the message.

## Performance

- **Git operations** like rebase no longer cause long pauses from repeated full-repo scans.


# 0.2.88 — 2026-07-06

## Features

- **Scrolling** feels smoother with better trackpad and wheel handling plus configurable speed and mode.
- **Session search** now returns tighter multi-word results and handles filenames and plurals better.
- **Session picker** now always searches conversation content for queries of two or more characters.
- **Tool call grouping** is now enabled by default, folding consecutive reads and searches into single rows.
- **Plugins tab** now supports `u` to update the selected plugin and shows non-blocking success feedback.
- **Reasoning effort** used for a session is now recorded in summary.json and conversation history.

## Bug Fixes

- **Session content search** now correctly indexes messages containing escape sequences like newlines and quotes.
- **Formatted links** now keep their link color when wrapped in bold, italic, or strikethrough.
- **Resuming sessions** no longer fails permanently when history files contain corrupted lines from interrupted writes.


# 0.2.87 — 2026-07-05

## Features

- **Subscription upgrades** are now detected automatically without restarting the CLI.
- **Bash permission prompts** now offer a "Never allow" choice that persists the deny rule.
- **New `/docs` slash command** opens How-to Guides picker, browses web docs, or jumps directly to a guide by title.
- **Per-model reasoning effort menus** are now configurable from the server and config.toml without a client release.
- **Finished thinking blocks** now fold into grouped tool-call rows when group_tool_verbs is enabled.

## Bug Fixes

- **--minimal** mode now always uses your terminal's own colors so text stays readable on any background.
- **Invalid fields** in [model.*] config blocks no longer cause the whole model to disappear from the picker.
- **File tools** no longer target paths with literal trailing newlines or whitespace from model output.
- **Fixed interactions** in no-freeform question modals so clicks below the last option no longer enter input mode.
- **Background tasks** now correctly wake the agent after a cancelled blocking wait instead of staying idle.
- **Copying quoted text** from rendered responses no longer includes the quote bar prefix in pretty mode.

# 0.2.86 — 2026-07-04

## Features

- **Voice language** setting now lets you pick speech-to-text language (including System) in the Editor settings.
- **Tab autocomplete** now suggests your next prompt as ghost text after each turn.
- **/usage** (and /cost) is now hidden for free and X Basic personal accounts.
- **Media generation** no longer hits per-session file limits; image and video byte budgets increased.

## Bug Fixes

- **--minimal** flag now shows in `grok --help`.
- **Session resume notifications** no longer appear when a workspace boots for the first time.
- **Claude-style Bash(cmd:*)** permission rules are now correctly translated to prefix matches.


# 0.2.85 — 2026-07-03

## Features

- **Pressing Enter** on an empty prompt now sends the top queued follow-up immediately while a turn is running.
- **Tool call grouping** can now be enabled via config.toml or settings to collapse consecutive read/search/list calls.
- **Consecutive tool calls** of the same kind can now be folded into a single row when group_tool_verbs is enabled.
- **Subagent conversations** now receive the same type-specific instructions in gateway chat as in the CLI.
- **Scheduled automation tasks** now show their header panel correctly in gateway chat sessions.
- **Promotional announcements** now appear with clickable CTAs and can be non-dismissible.
- **Subagents** now run in the background by default unless explicitly set to false.

## Bug Fixes

- **Permission prompts** now correctly wrap long bash commands while preserving structure and quotes.
- **Claude Code settings** with permissions.defaultMode are now correctly honored.
- **Project skills and commands** are now discovered even when their directories are gitignored.
- **Inline LaTeX math** with padding spaces now renders correctly instead of showing raw dollar signs.
- **Manual /recap** now works on the same turn and long auto recaps are hidden from view while still saved.
- **Always allow** for common commands like ls and git status now remembers just the command instead of extra arguments.


# 0.2.84 — 2026-07-03

## Features

- **Announcements** now update live during active sessions without restart or `/new`.
- **Hiding** an announcement no longer suppresses later criticals; new ones reappear automatically.
- **run_terminal_cmd** now requires a one-sentence `description` rationale in every invocation.
- **ask_user_question** timeout policy is now configurable in config.toml and `/settings`.
- **Ask-Question timeout** can now be toggled from `/settings` (Agent & Approval).
- **Thinking/reasoning blocks** are now shown by default while the model is working.
- **Critical announcements** now show a red title with a clickable [hide] button and aligned message.
- **Added remote_fetch option** under [features] in config.toml to disable all backend catalog and settings fetches for air-gapped environments.

## Bug Fixes

- **Images** pasted or read from GIF, BMP or TIFF files are now automatically converted so they work with image generation.
- **Queue panel** now shows action buttons on hover and the status bar displays a compact done/total task count.
- **Hook matchers** now correctly see the real MCP tool name instead of the internal dispatcher name.
- **Copy** now succeeds when running inside containers even when the terminal brand cannot be detected.
- **Tool result previews** no longer paint opaque panels in `grok --minimal`.
- **grok wrap** now correctly handles quoted strings and shell aliases.
- **Text selection** settings now correctly honor explicit keep_text_selection values even when legacy keys remain.
- **Fixed a freeze** that could occur when editing and sending the last message in the queue.
- **Fixed a startup crash** on minimal Linux systems lacking system CA certificates.

## Performance

- **Grep** now stops early on broad searches, returning faster results with far less memory use.
- **Idle CPU and memory** usage after long sessions or resume is now dramatically lower.


# 0.2.83 — 2026-07-02

## Features

- **Critical announcements** now appear in a top banner during active sessions with a hide command.
- **Pasting the same text again** next to a paste chip now expands the chip into editable text instead of duplicating it.
- **Paste preview** now shows a hint explaining how to expand the chip.


# 0.2.82 — 2026-07-02

## Features

- **Managed connectors links** now include the team ID when opening from a team session.
- **AGENTS.md files** are now discovered and shown for workspace/hub sessions.
- **Chat conversation titles** from the gateway are now shown in the sidebar.
- New `/effort` slash command changes reasoning effort on the active model.
- Double-click a pasted text chip to expand it into editable text.

## Bug Fixes

- **Skill descriptions** are now recovered correctly even when frontmatter YAML is malformed.
- **[Esc] hint** in /btw panels now stays visible even on narrow terminals.
- **Background monitors** now wake the agent on natural exit the same way bash tasks do.
- Long option labels in question prompts are now always visible instead of disappearing when unfocused.
- Pasted text preview now appears immediately after inserting a paste chip.
- Hex color codes now render as colored dots with no extra space.
- Pressing voice on the welcome screen now starts a new session.

# 0.2.81 — 2026-07-01

## Features

- **Chat sessions** no longer send workspace binding hints that belong to the backend.
- **New stream transforms** let hosts hide, unwrap, or rewrite tool calls for display without affecting agent transcripts.
- **cancel()** now accepts a timeout so a stuck turn cannot hang the session forever.
- **Run tool blocks** now show the model's description as the main title when provided.
- **Hex color codes** in prose now render as colored dots on truecolor terminals.
- **New setting** "Show thinking blocks" controls whether agent reasoning is visible in the scrollback.
- **Spinners** now show the description or short command of the task being waited on when available.

## Bug Fixes

- **Fixed sessions** that remained stuck on the thinking indicator after the model finished responding.
- **Mermaid diagrams** now correctly display angle brackets and symbols instead of literal HTML entities in labels.
- **Recap requests** no longer trigger context-length 400 errors on long conversations.

## Performance

- **Grep searches** now time out after 20 seconds by default (60s on WSL) instead of always waiting 60s.

# 0.2.80 — 2026-07-01

## Features

- **Command timeouts** can now be configured per-session with a foreground-only ceiling.
- **Background tasks and TODO lists** now survive compaction and remain visible to the model.
- **Voice dictation** STT feature: uses Ctrl+Space or F8, with optional hold-to-talk on supported terminals.
- **Contextual hints** can now be toggled individually for undo, plan mode, and image input.

## Bug Fixes

- **Subagent dialogs** now reliably show full transcripts on open and reopen.
- **Recap blocks** now copy only the summary body, not the header label.
- **Vim navigation keys** now type into dashboard prompts; modals properly handle Esc/Left.

## Performance

- **Network connections** are now more resilient to proxy/LB drops.

# 0.2.79 — 2026-06-30

## Features

- **Contextual hints** now show shortcuts like plan mode or clipboard paste when relevant.
- **Graceful shutdowns** now allow interrupted turns to resume with a configurable pause budget.
- **Grok.com chat sessions** now integrate fully with the gateway bridge for model catalog and resume.

## Bug Fixes

- **Question prompts** now time out after 6 minutes instead of blocking forever.
- **Fixed a crash** that could occur during conversation integrity repairs while a turn was active.

## Performance

- **Compaction** can now run part of its work in the background before it blocks the session.

# 0.2.78 — 2026-06-30

## Features

- **Chat sessions show the grok.com model catalog** in the picker.

## Bug Fixes

- **Tabs pasted into the prompt** now align correctly with proper cursor positioning.
- **Pasting images into dashboard peek replies** now works and survives turn cancellation.
- **Links in /btw panels** are now clickable and highlight on hover.
- **Prompt history is now saved** even on fast Ctrl+C quit.
- **Stuck scrollback text selection** can now be cleared with Esc or any non-drag input.
- **LaTeX math now renders** inside markdown tables in the TUI.
- **Background shell commands** started by the agent are now cleaned up when the CLI exits.

## Performance

- **`grok update`** downloads have a longer timeout.


# 0.2.77 — 2026-06-30

## Features

- **Pasting images** from the local clipboard now works when running commands through `grok wrap`.
- **Turn status spinner** now shows what the agent is waiting on (response, subagent, task output, etc.).
- **Double-click word selection** is now a discoverable option in the Text selection setting and stays in sync with highlight behavior.

## Bug Fixes

- **Credit limit errors** now show clearer upgrade or buy-credits messaging based on billing type.


# 0.2.76 — 2026-06-30

## Features

- **Auto permission mode** is now added to the top of Shift+Tab cycles and enabled by default in settings.
- **grok agent stdio** now checks for updates in the background like other modes.

## Performance

- **Idle sessions** no longer send repeated empty frames to the terminal, reducing CPU usage in the terminal emulator.


# 0.2.75 — 2026-06-29

## Features

- **Prompt history** (Up arrow / Ctrl+R) now shows only the current session's prompts, with the newest selected at the bottom.

# 0.2.74 — 2026-06-29

## Features

- **Esc now cancels a running turn immediately**; double-Esc clears prompt or opens rewind when idle.
- **grok wrap** now shows copy success over SSH and suggests native drag-select when paste fails.

## Bug Fixes

- **Clipboard copy** now succeeds reliably on Wayland and KDE desktops instead of showing false positives.


# 0.2.73 — 2026-06-28

## Features

- **Keep text selection highlight** setting added so drag selections stay visible until dismissed.

## Bug Fixes

- **Doubled lines** after tab switches or focus changes in tmux or editor terminals are now healed.
- **Clipboard copy** now only shows success when the pasteboard actually received the text via a trusted path.


# 0.2.72 — 2026-06-28

## Bug Fixes

- **No longer triggers browser login** at startup when an API key is already configured for inference.


# 0.2.71 — 2026-06-27

## Bug Fixes

- **Fixed `grok agent stdio` hangs** on Windows when used with persistent clients such as VS Code.


# 0.2.70 — 2026-06-27

## Breaking Changes

- **Added `grok wrap`** to run any command with local clipboard support.

## Features

- **Ctrl+4** now toggles the prompt queue on local macOS VS Code terminals.

## Bug Fixes

- **Session recaps** (/recap and return-from-away) now show the full summary instead of being cut off mid-sentence.
- **Vim mode** now focuses the prompt when you press / on a brand-new empty session.
- **Fixed `grok agent stdio` startup hangs** on Windows when used with persistent ACP clients such as VS Code.
- **`/mcps` list** no longer shows stale disabled entries when managed gateway tools are enabled.
- **Mermaid diagrams opened via [Open Image]** now render at higher resolution instead of terminal size.
- **Pressing `r` in scrollback** no longer accidentally rewinds the session.
- **Shortcuts cheatsheet** now shows Ctrl+X on terminals that cannot deliver Ctrl+.
- **Folder trust prompts** no longer re-appear for every standalone worktree clone.
- **Reasoning effort** no longer silently resets from a user-chosen value after catalog refreshes.
- **Fixed clipboard copy** inside editor terminals nested in tmux by emitting plain OSC 52.

# 0.2.69 — 2026-06-26

## Features

- The agent dashboard now shows each agent's model and mode in the peek panel, lets you cycle modes with Shift+Tab, collapses the Inactive section by default, and hides older idle agents behind a "N more" row.
- Tool usage cards for search, directory listing, file deletion and glob now render as distinct typed cards instead of generic MCP entries.
- The keyboard shortcuts help now shows richer descriptions and correctly scrolls wrapped text in the detail view.
- You can now pass --json-schema to grok -p and receive a validated JSON object instead of free text.
- **Ctrl+L** now interjects mid-turn in VS Code, Cursor, Windsurf, and Zed terminals.

## Bug Fixes

- Local plugins installed from your home directory are now automatically refreshed when you start a session, so new agents or skills added to the source appear immediately.
- The /context command now reports the same number of tool definitions that are actually sent to the model.
- In vim mode the agent dashboard peek no longer steals keyboard focus from the list, so j and k keep moving between agents.
- **/sessions** on the agent dashboard no longer freezes the interface.
- **Dashboard** now focuses the overview list immediately when agents exist.

# 0.2.68 — 2026-06-26

## Features

- **MCP servers** from host integrations can now be added, replaced, or removed without restarting the session.
- **Agent-run terminal commands** now set `GROK_AGENT=1` so host tools can tell them apart from interactive shells.

## Bug Fixes

- **Attached images** are now saved to real disk paths so the model can read them in any terminal.
- **/resume** now selects the correct model when a saved model name is ambiguous.
- **Slash and completion menus** no longer crash if the terminal is resized while open.


# 0.2.67 — 2026-06-25

## Features

- **Added --json-schema** flag for headless mode to constrain model output to a supplied JSON Schema.
- **Idle detection** can now ignore background tasks when the env flag is set (off by default).

## Bug Fixes

- **Preview panes** no longer hibernate while actively viewed or polled.
- **Manual /rename** now persists correctly and appears in /session-info even after auto title generation or resume.

## Performance

- **Find and grep** now transparently use faster bfs and ugrep binaries when present in the harness.


# 0.2.66 — 2026-06-25

## Features

- **Custom sandbox profiles** can now kernel-deny specific files and directories for reads/writes.
- **Marketplace plugins** in subdirectories of a git repo can now be installed and loaded correctly.
- **Folder trust prompt** now appears before starting a session when the feature is enabled.
- **Preview panes** no longer hibernate while actively viewed.
- **Keyboard shortcuts help** now expands inline for individual entries instead of only sections.
- **Idle detection** can now ignore background tasks when the env flag is set (off by default).
- **Sandbox deny lists** now accept glob patterns like **/*.pem** in addition to exact paths.

## Bug Fixes

- **Local MCP servers** now auto-recover after disconnects or session expiry.
- **OIDC sessions** with GROW_API_KEY present no longer lose refresh on idle.
- **Inline video previews** now show an install command only when the package manager is on PATH.
- **list_dir** now reliably shows all immediate child directories even inside large monorepos.
- **Clicking a model** in the dashboard /model dropdown no longer opens the wrong session.
- **Strikethrough** now only applies to ~~double tildes~~; single ~tildes~ render literally.
- **Session cycling** with Ctrl+[ / ] now switches from the session you are currently viewing.
- **Prompt history** (Up / Ctrl+R) now shows the complete recent list instead of a scrambled partial one.
- **Authentication** now correctly prefers the session method when both API key and cached token are present.
- **xychart-beta** diagrams with category labels now render correctly as images.


# 0.2.65 — 2026-06-24

## Features

- **grok -w --ref <branch>** now creates worktrees based on the specified ref instead of HEAD.

## Bug Fixes

- **Unidentified Windows consoles** are now treated as Windows Terminal for capability decisions.
- **Esc** in the dashboard input now moves focus to the list without clearing your typed draft.
- **Copying** a tool header now copies just the path or command, not the Read/Run label.
- **Execute activity** lines and headers no longer repeat a redundant cd into the session directory.
- **Inline video previews** now show an install hint instead of a spinner when ffmpeg is missing.

## Performance

- **Headless and stdio sessions** no longer start unnecessary filesystem watchers, saving CPU and IO.
- **Scrolling** feels more responsive in VS Code, Cursor, and Windsurf integrated terminals.


# 0.2.64 — 2026-06-24

## Features

- **Dashboard** now displays the current directory and branch; click or press Ctrl+L to change location, or Ctrl+W to dispatch new agents into fresh git worktrees.
- **/recap** now appears as a collapsible tool-style block with a loading spinner while generating.

## Bug Fixes

- **Dashboard** arrow keys open agent details and exit overlays; closing an agent now selects the neighboring row.
- **/usage** command and credit warnings are now hidden for API-key authentication.
- **MCP servers** from your user config no longer appear labeled as project-scoped when running from your home directory.


# 0.2.63 — 2026-06-23

## Bug Fixes

- **Fixed hook matchers** so pipe-list and alias patterns no longer silently over-match unrelated tool names.


# 0.2.62 — 2026-06-23

## Features

- **Hosts can now register hooks** over the agent connection instead of only on-disk files.
- **Prompt and /usage warnings** now correctly reflect prepaid credits and auto top-up status.
- **ACP clients can now detect** when a terminal is busy running a foreground process.
- **TODO list** remains visible to the model after compaction so it can continue working on pending items.
- **/recap** is now available by default — it generates a quick summary of your current session so you can catch up on what's happened so far.

## Bug Fixes

- **MCP server connections** no longer time out during slow cold starts of stdio servers that download dependencies.
- **File paths containing spaces** (e.g. macOS app bundles) are now correctly turned into clickable hyperlinks in the terminal.
- **Resume** now correctly picks the most recently active session instead of one that only had metadata updates.
- **/goal** slash command now appears in the menu on the welcome screen before any prompt is sent when the feature is enabled.
- **Session picker** no longer shows a stale row highlight when keyboard focus moves to the search bar.
- **Usage percentages** in /usage and warnings now match backend flooring and show pay-as-you-go limits when applicable.
- **Team accounts** can now list sessions after re-login; previously returned 403 on conversations API.


# 0.2.61 — 2026-06-22

## Features

- **Closing a terminal tab** with a running process now shows a confirmation dialog instead of killing it immediately.
- **/usage** now shows prepaid credits balance and auto top-up status.
- **Clipboard copy** on Wayland now also tries wl-copy; per-leg outcomes are now logged for diagnostics.
- **Goal mode toggles and limits** can now be set in config.toml under the [features] table.
- **All /goal options** (toggles, limits, role models) are now configurable together in a [goal] table.
- **Clipboard copies** from VS Code over SSH now warn when non-ASCII text may be garbled.

## Bug Fixes

- **Focus reports** no longer leak as literal text when split across reads over SSH.
- **Focus events** now fire correctly for SSH-split focus reports.
- **Boolean tool flags** now accept "true"/"false"/"yes"/"no"/1/0 strings and numbers in addition to native booleans.
- **Session last-active timestamps** and message counts no longer regress under concurrent writers.
- **iTerm2** now always uses text/metadata image fallback instead of broken OSC 1337 overlays.
- **Model switches** no longer leave the prompt queue stuck after a reconnect.
- **Closing a terminal tab** with a running process no longer shows a confirmation dialog.
- **Custom agent profiles** now correctly use the harness required by their pinned model.
- **Subagents** under custom profiles now adopt the correct harness from the parent's model.
- **Changelog and release-notes** modals now scroll with the mouse wheel and arrow keys.


# 0.2.60 — 2026-06-21

## Features

- **/resume** now shows sessions from your current working directory's repo at the top of the list.
- **Too-wide Mermaid diagrams** now show a hint below the fallback box pointing to the Open Image button.
- **Cancel behavior for running subagents** can now be set to always stop or always continue in config.toml.

## Bug Fixes

- **Compaction** no longer hangs indefinitely when the summarizer stream stalls after the server has finished.
- **Slash command completion** now shows consistent suggestions and remembers recently used commands.
- **Queued prompts** now reappear reliably after deleting the last item and re-queuing.
- **Headless sessions** no longer produce authentication error noise from unauthenticated MCP servers.
- **Mermaid flowchart labels** with long identifiers are now kept whole instead of being cut mid-word.
- **Cmd+Backspace** now deletes only from the cursor to the start of the line instead of clearing the whole prompt.
- **Inline Mermaid previews** now break long identifiers at word boundaries instead of mid-segment.
- **Signed git commits** no longer corrupt the TUI by letting pinentry draw over the screen.
- **Arrow keys** now move the prompt cursor or open history while a /btw answer panel is visible.
- **Long option descriptions** in question prompts now expand fully when the row is focused.

## Performance

- **Large MCP tool results** are now truncated inline and saved to disk to avoid unnecessary context compaction.


# 0.2.59 — 2026-06-19

## Bug Fixes

- **Session recaps** no longer display doubled labels and manual recap now correctly suppresses the next automatic recap.


# 0.2.58 — 2026-06-19

## Bug Fixes

- Terminal command output files are now capped at 5 GB during execution and truncated to 64 MB after the process exits.
- Interjection messages now display the actual user text instead of a generic header.
- The legacy `agent` command is now kept in sync with `grok` after running `grok update`.
- Headless (`grok -p`) runs now wait for background tasks and subagents to finish before exiting.


# 0.2.57

## Features

- Improved resilience to network blips during long responses by resuming instead of failing the turn.
- **`grok plugin install <name>`** now resolves plugins from registered marketplaces instead of only local paths.

## Bug Fixes

- Fixed cases where long-running conversation compaction could hang indefinitely.
- Notification hooks now fire only for real user-attention events and no longer trigger constantly during tool use.
- Fixed literal display of HTML entities such as &lt; and &gt; in responses and tool output.
- **Typing `[`** in the pager prompt no longer appears delayed.
- **Copy** now tries all available Linux clipboard tools so paste works reliably in more terminals.


# 0.2.56

## Features

- **resume_from** now continues a finished sub-agent in place instead of forking a new conversation.
- **grok sessions delete <id>** command now lets you permanently remove a session from the CLI.

## Bug Fixes

- **MCP server connections** no longer get torn down during rapid config reloads.
- **Stale leader processes** are now cleaned up when leader mode is disabled via config or remote settings.
- **Sandbox profile** is now preserved when resuming sessions so commands continue to work as before.
- **list_dir** now shows more relevant files when a large directory appears early in alphabetical order.
- **Cancel button** in turn status always shows [stop]; queue pane highlight now follows theme changes.
- **grok quit** no longer hangs when background git or network tasks are slow.
- The token count shown after auto-compaction now matches the context bar exactly.
- The git branch icon now renders correctly in iTerm2 without a Nerd Font.
- **list_dir** now gives clearer guidance when a directory is too large, using the actual tool names available in your session.
- **Ctrl+Enter** now sends the prompt when the agent is idle (same behavior as Enter).
- **resume_from** now correctly continues a sub-agent in the same working directory it was using before.
- Files with non-ASCII names (e.g. Chinese) no longer crash the session when plan mode checks for markdown.
- Session lists (welcome screen, /resume, grok sessions list) are now sorted by the same activity time shown in the UI.
- **Fixed bash tool failures** when models send numeric arguments such as timeout as JSON strings instead of numbers.
- **Prevented crashes** during bash command output streaming when building progress frames.
- **Disabled inline image rendering** on iTerm2 terminals where scrollback overlays cannot be supported.

## Performance

- Fast tools like grep now show as completed immediately even when other tools in the same round are still running.
- Long sessions that display inline images no longer grow to multi-GB memory usage.


# 0.2.55

## Features

- **Added option** to fully disable the hunk tracker via --hunk-tracker-mode, GROK_HUNK_TRACKER, or config.

## Bug Fixes

- **Windows install scripts** now download and run cleanly via irm | iex without spurious BOM errors.
- **Tables and wide content** no longer leave stray characters next to timestamps in the scrollback.
- **Mermaid diagrams** now render node labels cleanly without HTML tags or raw markdown syntax.
- **MCP servers using HTTP** now recover automatically after temporary connection drops instead of becoming permanently unavailable.
- **Very long sessions** can now scroll all the way to the bottom of the conversation history.


# 0.2.54

## Features

- **Rewind** now works end-to-end across conversation and file state with proper CAS handling.

## Bug Fixes

- **Git branch icons** now render correctly on Windows without Nerd Fonts.
- **Mermaid diagrams** now render inline without the model suggesting external viewers.
- MCP connection errors now show the actual failure reason to the model.
- MCP servers with noisy stdout no longer disconnect unexpectedly.
- **Usage warnings** now always display "Usage left: N%" instead of varying between "Free credits left" and "Credits left".
- **Window title** no longer flashes or oscillates during permission prompts while the terminal window is focused.

## Performance

- **Fixed pager freezes** and 100% CPU usage when rendering very long agent reasoning outputs with thousands of styled spans.


# 0.2.53

## Bug Fixes

- Minor bug fixes.

# 0.2.52

## Features

- **Tool auto-approval (YOLO)** state is now tracked end-to-end in server-side agent sessions.
- **ER diagrams** now render as entity boxes with attributes and relationships in the TUI.
- New "Respect manual folds" setting keeps hand-expanded blocks stable while content streams in.
- **Ctrl+X** now stops running turns or closes sessions from inside the agent detail view.
- **Grok** can now export usage metrics and events to your own OpenTelemetry collector when enabled.
- **WezTerm users** now receive guidance when Shift+Enter fails because kitty keyboard protocol is disabled.
- **Long-running sessions** now tell the model when the local calendar date changes past midnight.
- **Agent Dashboard** now works without leader mode and shows local idle sessions from disk.

## Bug Fixes

- **Fixed oversized session replay logs** that prevented large sessions from loading.
- **MCP server connections** no longer flood reconnects on repeated stream errors.
- **ZDR and team upload flags** are now populated immediately on login instead of only after background refresh.
- **Mermaid PNG export** now handles quoted cardinalities in class diagrams and readable ER rows on dark theme.
- **Skill catalog** no longer shows duplicate "Use when:" labels and check-work skill now prompts the model to read its instructions.
- Compaction now rejects overly-short summaries that would discard real conversation state.
- Background tasks no longer emit spurious failure messages when a session is resumed.
- **Fixed Windows path handling** so external tools and model prompts receive clean paths without \?\ prefixes.
- **Images and media** no longer remain visible when switching from an agent view to the dashboard.
- **Clipboard paste** (Ctrl+V) now works for images on pure Wayland sessions.
- **Modals** such as /sessions no longer crash on narrow terminals.
- **ptyctl resize** now correctly notifies the child process.
- **Concurrent updates** to the same version no longer fail with permission or EEXIST errors.
- **Mermaid diagrams** containing CJK or other non-Latin text now render correctly instead of tofu boxes.
- **`grok dashboard`** now reliably opens the dashboard instead of silently falling through to a normal session.
- **Sessions** no longer remain blocked forever after a transient model catalog outage during reconnect.
- **Cancel** no longer leaves the interface stuck on "Cancelling…" after lost responses during reconnects.
- **Forked sessions** now retain the parent's full pre-compaction transcripts instead of only the compacted summary.
- **web_fetch** errors on GitHub hosts now recommend using the gh CLI when internal access is blocked.
- **MCP server connections** no longer hang when stdio servers emit undecodable lines.
- **Ctrl+C cancels** now complete in under 50 ms instead of blocking for seconds.
- **Repeated varied edit failures** on one file no longer trigger doom-loop warnings or terminations.

## Performance

- **Compaction** now reuses cached prompt prefix instead of full prefill.

# 0.2.51

## Breaking Changes

- **`grok mcp add`** now accepts positional arguments (e.g. `grok mcp add filesystem -- npx ...`), supports --scope project, and adds -e/-H flags for env/headers.

## Features

- **Mermaid flowcharts** now render subgraph blocks as titled frames with correct internal and cross-boundary edges.
- **Class diagrams** in Mermaid now render as proper UML boxes with attributes, methods and inheritance arrows instead of raw source.
- **Permission prompts** now accept a double-click on an option to submit it, matching the existing Enter and number-key shortcuts.
- **New /code-review slash command** now ships with the CLI and is always available.

## Bug Fixes

- **Plan mode exit reminders** no longer appear after the model has already started implementing the plan.
- **Expanded thinking blocks** in scrollback now remain expanded when the agent finishes them.
- **`grok update`** no longer downloads the same binary twice when multiple updaters or leader checks run concurrently.
- **Background task IDs** after /compact are now shown verbatim so the model can reference them correctly in later tool calls.
- **Typing /** while scrollback is focused now focuses the prompt and opens the slash-command dropdown.
- **Dashboard empty state** is now a single hint line; dispatch and peek placeholders appear only when unfocused.
- **Fixed memory leaks** that could cause the CLI to use tens of gigabytes during long sessions with many tool calls.
- **Login on SSH or headless machines** now tells you when the browser cannot be opened automatically and shows the URL to visit manually.
- **Fixed git clone failures** on Windows when the CLI tries to clone marketplace plugins into ~/.grok.

## Performance

- **Large code blocks** inside lists no longer cause multi-second UI stalls while streaming responses.


# 0.2.50

## Features

- **Mermaid flowcharts** now render edge crossings clearly instead of fusing unrelated connections.

## Bug Fixes

- **Sequence diagrams** with activate, autonumber, par, and more now render instead of showing parse errors.
- **MCP servers menu** and slash commands now work when starting grok outside a project directory.
- **Ctrl+W** in the prompt now deletes whole words like bash instead of stopping at punctuation.
- **Login** no longer quits when an authentication code contains the letter q.


# 0.2.49

## Features

- Marketplace plugin listings now show skills, MCP servers, and commands when the catalog is published.
- Mermaid flowcharts now render with fewer avoidable edge crossings.
- **stateDiagram** mermaid blocks now render as Unicode diagrams instead of source fallback.

## Bug Fixes

- **Skill reloads** no longer corrupt active tool calls or produce duplicate results in the conversation.
- **grok --resume** now correctly finds the real session instead of failing on empty image-only folders.
- Pasted images and relative paths now use the correct directory when resuming a session created elsewhere.
- **Mermaid flowcharts** now correctly render node groups, arrow endings, self-loops and line styles.
- **Fixed** "unknown session id" errors that occurred after the leader process crashed or was killed.
- **Pasted images** now survive interjections and queue edits instead of being dropped.
- **Managed MCP connectors** (Slack, Linear, etc.) now appear correctly when using leader mode.


# 0.2.47

## Features

- **stateDiagram** Mermaid blocks now render as diagrams instead of source fallback.

## Bug Fixes

- **Pasted images** now survive interjections and queue edits instead of being dropped.
- **Managed MCP connectors** (Slack, Linear, etc.) now appear correctly when using leader mode.


# 0.2.46

## Features

- **Mermaid flowcharts** now render with fewer avoidable edge crossings.

## Bug Fixes

- **Fixed `grok --resume`** failing on empty image-only session folders left by cross-directory pastes.
- **Fixed pasted images** and relative paths using the wrong directory after cross-cwd resume.
- **Fixed Mermaid flowcharts** that silently rendered wrong diagrams for & groups, circle/cross endings and self-loops.
- **Fixed zsh tab-completion** for subcommands after the optional prompt argument was added.
- **Fixed "unknown session id" errors** after the leader process crashed or was killed.
- **Fixed repeated auto-compaction attempts** when the session is credit-blocked or auth is non-refreshable.

## Performance

- **Parallel tool calls** on the same path (multiple greps etc.) now execute concurrently.


# 0.2.45

## Features

- **Mermaid diagrams** now render to images when you click Open in a code block (on by default).

## Bug Fixes

- **Fixed** rare conversation corruption when skills changed while a tool call was still running.
- **Fixed** `grok --resume` failing on empty image-only session folders left by cross-directory pastes.
- **Fixed** pasted images and relative paths using the wrong directory after resuming a session from another folder.
- **Welcome screen logo** no longer renders as invalid characters on legacy Windows command prompts and PowerShell.
- **Fixed** "unknown session id" errors that occurred after the leader process crashed or was killed.


# 0.2.44

## Features

- **K/J** keys now snap the viewport to the top of previous or next assistant responses.
- **J/K** (vim mode) now navigate between assistant responses in scrollback.
- **sequenceDiagram** mermaid blocks now render as Unicode lifeline diagrams instead of source fallback.

## Bug Fixes

- **Interjecting** while editing a queued prompt no longer strands the composer or blocks the queue.
- **Mid-turn interjections** now appear as separate user messages instead of being appended to tool results.
- **Project MCP config** touches no longer trigger repeated reload storms.

## Performance

- **Inference requests** recover faster from silent engine stalls instead of waiting the full idle timeout.


# 0.2.43

## Bug Fixes

- **ask_user_question** tool can now be enabled in allowlists without requiring plan-mode tools.
- **Shift+Tab** mode cycling (Normal → Plan → Auto-Approve) works again in the agent view.
- **Ctrl+C** now cancels a blocking `grok update` cleanly instead of leaving an orphaned download repainting the terminal.


# 0.2.42

## Bug Fixes

- **ask_user_question** tool can now be enabled in allowlists without requiring plan-mode tools.
- **MCP servers** provided at session start now persist across config hot-reloads.


# 0.2.41

## Features

- **Compaction completion message** now shows the before → after token reduction instead of only the final count.

## Bug Fixes

- **Fixed token count after compaction** so the displayed number no longer jumps back up on the next model response.
- **Fixed plugin skill loading** when a manifest lists skill directories directly instead of a parent skills/ folder.

## Performance

- **Fixed memory context injection** on resume so the prompt prefix stays byte-stable and KV cache is preserved.


# 0.2.40

## Features

- **`grok --debug`** now produces per-session log files under ~/.grok/debug/ even with a leader process.

## Bug Fixes

- **Doom-loop warnings** now correctly describe cycles and distinct edit failures instead of claiming identical arguments.
- **Model list changes** from config or cache now appear in already-connected TUI and IDE clients without restart.


# 0.2.39

## Features

- **run_terminal_cmd** can stream live stdout/stderr chunks when the workspace flag is enabled.
- **/session-info** now displays the current turn index.
- Server-synced and bundled skills are now discovered from launcher-injected directories.
- **Background `&` operator** is now allowed by default in terminal commands.

## Bug Fixes

- **Resumed subagents** no longer loop forever during auto-compaction on large context windows.
- **Background task** descriptions and & rejection messages now correctly name the real parameters.
- **Doom-loop detection** no longer falsely triggers on distinct failing tool calls.


# 0.2.38

## Features

- **Watching status line** now appears when background monitors, loops, or subagents can wake the agent.

## Bug Fixes

- **Default model selection** now correctly chooses the intended entry when multiple models share a slug.
- Minor bug fixes


# 0.2.37

## Features

- **MCP tool result queries** now list only command-line tools actually present on your system.
- **`grok update`** now restarts any older running leader so all clients get the new binary.
- **Long-running bash commands** that hit the timeout are now moved to the background by default instead of killed.

## Bug Fixes



# 0.2.36

## Features

- **Large MCP tool results** are now saved with the correct extension and the model receives better hints for querying them.

## Bug Fixes

- **Fixed false-positive doom-loop terminations** when many parallel tool calls fail together in one batch.
- **Fixed a crash** that could occur during auto-compaction when resuming a session containing reasoning content.


# 0.2.35


# 0.2.34

## Features

- **`grok login`** now defaults to device code flow, which works reliably in SSH, WSL, VPN, and browser-restricted environments.

## Bug Fixes

- **Fixed a hang during auth refresh.**


# 0.2.33

## Bug Fixes

- **Fixed duplicate turn output** when attaching a second client to an active leader session.
- Fixed **Send now** on queued prompts.


# 0.2.32

## Features

- **Slash commands** from project plugins now appear correctly in every open conversation after a plugin change.

## Bug Fixes

- **Prompts submitted rapidly** now stay in correct submission order in the queue.

## Performance

- **Grep searches** on large repositories are now substantially faster and no longer hit the 60-second timeout.


# 0.2.31

## Bug Fixes

- **Marketplace skills** without proper descriptions are now hidden from listings instead of flooding the model with tables.
- **Prompts submitted rapidly** now stay in correct submission order in the queue.

## Performance

- **Grep searches** on large repositories are now substantially faster and no longer hit the 60-second timeout.


# 0.2.30

## Features

- A new plugin install suggestion appears above the prompt when you type a known marketplace plugin name or domain.

## Bug Fixes

- **Trace uploads** and remote session restores now succeed with a deployment key and no browser login.
- **Resumed sessions** no longer pad the sticky prompt with empty rows; cancelling a turn now keeps the rest of the prompt queue intact.
- Cancelling a running prompt no longer leaves the interface stuck on the cancelling spinner.


# 0.2.29

## Bug Fixes

- **`/rewind`** before a compaction boundary no longer leaves later prompts in context.

## Performance

- **Resuming large sessions** is now substantially faster with no data loss.


# 0.2.28

## Bug Fixes

- **Images** read via read_file are now downscaled even when small in bytes but large in pixels.
# 0.2.27

## Features

- **Image and video generation** tools now include the saved filename and session folder in their output.

## Bug Fixes

- **Monitor output** no longer appears as raw XML in the conversation view during leader sessions.
- **Windows commands** containing `&` are no longer incorrectly rejected by `run_terminal_cmd`.
- **Python -c** save-to-file reminder now suggests correct commands on Windows.


# 0.2.26

## Bug Fixes

- **Large pasted content** no longer triggers context-window errors or breaks compaction and memory flush.
- **API-key users** can now run `grok agent --leader` without forced interactive login or timeouts.
- **Compaction** no longer retries endlessly on credit, size, or auth failures; shows a clear message instead.
- **Windows PowerShell and cmd.exe** no longer falsely reject commands containing `&`.
- **web_fetch** no longer crashes the CLI on pages whose root element matches a cleaning selector.
# 0.2.25

## Bug Fixes

- **Session titles** now generate reliably even for very long initial messages.


# 0.2.24

## Bug Fixes

- Minor bug fixes
# 0.2.23

## Features

- **Leader sessions** can now be viewed and controlled from multiple clients with a live dashboard.
- **Sessions** can now be deleted directly from the /resume history picker.

## Bug Fixes

- **MCP plugin servers** with bundled OAuth client IDs now authenticate correctly.
# 0.2.22

## Bug Fixes

- **Authentication errors** with static API keys now surface a clear error instead of hanging the turn.


# 0.2.21

## Features

- **allowed_models** in config.toml now restricts which models appear in the picker and `/model` command.

## Bug Fixes

- **Code navigation** now returns correct results for secondary project windows with different working directories.
# 0.2.20

## Bug Fixes

- **MCP servers** declared in both a plugin's .mcp.json and plugin.json are now registered instead of dropped.
- **Git operations** now correctly target the repository for each session's working directory.
# 0.2.19

## Features

- **Monitors** now appear labeled in background-task reminders after compaction and can be terminated by name.

## Bug Fixes

- **Reading images** with text-only models no longer triggers repeated 400 errors that brick the session.


# 0.2.18

## Features

- **Official xAI plugin marketplace** now appears automatically in the Marketplace tab on first launch.
- **Image and video generation** now use api.x.ai directly for all users.
- **New image-to-video and reference-to-video tools** are now available for generating videos from images.
- **New imagine skill** provides prompt-craft and workflow guidance for image generation and editing tools.

## Bug Fixes

- **image_edit** now correctly resolves pasted or attached images referenced as [Image #N].
- **Background subagent completions** are no longer reported twice when the agent is idle.
- **Subagents** now use the same model as the parent session by default.


# 0.2.17

## Features

- **Image and video generation** tools now emit structured paths so the pager renders media without regex scraping.
- **Compaction summaries** now use a more detailed structure that improves recovery after context reset.
- **image_gen** can now be enabled via the harness model using [features] in config.toml or the GROK_IMAGE_GEN_HARNESS env var.
- **Improved config refresh** on new sessions from the shell.

## Bug Fixes

- **--restore-code** no longer detaches the source repository when resuming a forked-worktree session from a different directory.
- **Read tool** string coercion bug fixes.
- **ICO images** pasted or read from disk are now automatically converted to PNG before being sent to the model.
# 0.2.16

## Features

- **New segments compaction mode** writes per-segment markdown files that the model can read to recover pre-compaction detail.
- **Claude and Cursor compatibility scanning** (skills, rules, AGENTS.md) can now be toggled individually via env vars or config.toml.
- **grok inspect** now shows the resolved on/off state and source for every Claude/Cursor compatibility toggle.
- **Cursor MCP servers and hooks** are now discovered and can be disabled independently via GROK_CURSOR_MCPS_ENABLED / GROK_CURSOR_HOOKS_ENABLED.

## Bug Fixes

- **Streaming tool output** (bash/write_file) now renders completely in the pager instead of only the latest chunk.
- **Streaming bash tool output** now appears correctly in the pager scrollback.
- **Routing a native tool** (e.g. scheduler_create) through use_tool now gives a clear corrective error instead of an unrecoverable loop.
- **"Starting session..."** spinner no longer gets stuck when zero MCP servers are configured.
- **Subagents** now use the correct harness after switching models mid-session.
- **Fixed long startup delays** when an external auth provider binary hangs or fails.
- **Subagent conversations** no longer receive unrelated monitor events or background task completions from the parent.
- **The /loop command** now accepts natural-language intervals instead of always defaulting to 10 minutes.
- **Fixed blank output** on completed bash or code-execution cards after shell restart or reconnect.

## Performance

- **Large pasted images** no longer bust the prompt cache or exceed the 50 MiB request limit.


# 0.2.15

## Features

- **Permission prompts** now remember your last choice across tools and let you configure the first-prompt default in config.toml.


# 0.2.14

## Features

- **Generated images and videos** can now be opened directly from the terminal UI via buttons or clicks.
- **Background tasks panel** now groups items, supports collapsible sections, and has clearer styling for monitors and loops.

## Bug Fixes

- **Session titles** are now generated reliably using a fixed default model.
- **--permission-mode** now correctly overrides the permission_mode setting from config.toml when launching sessions.


# 0.2.13

## Bug Fixes

- Miscellaneous bug fixes


# 0.2.12

## Features

- **Computer connection status** now shows a connecting pill during terminal session initialization.
- **/check** and subagents now read and follow full AGENTS.md rules from the repo.

## Bug Fixes

- **--max-turns** now correctly counts tool-use cycles instead of total messages.
- **@-mention file search** now works again for local agent sessions.
- **Rendered images, files, and citations** now replay correctly in chunk-mode history.
- **`/context`** now displays the correct auto-compact threshold for the active model instead of always 85%.
- **Model responses** are no longer silently dropped when the gateway emits legacy channel values.
- **Prompt responses** no longer resolve before the turn's final output chunks reach the client.


# 0.2.11

## Bug Fixes

- Minor bug fixes
# 0.2.10

## Features

- **`/check`** has been renamed to **`/check-work`**; old command continues to work during transition.

## Bug Fixes

- **Images smaller than 8×8 pixels** are now rejected with a clear message instead of producing blocky results.


# 0.2.9

## Features

- **Added --device-code** as alias for device authentication and improved headless auth error messages.


# 0.2.8

## Features

- **New /login** slash command lets you re-authenticate from within a session without quitting.
- **Compaction summaries** now include the full transcript path so the model can reference prior details.
- **Cursor skills and rules** are now discovered alongside Grok and Claude directories.

## Bug Fixes

- **Fixed monitor tool** schema to show the correct 10-hour default timeout.
- **Fixed a panic** that could occur when installing marketplace plugins.


# 0.2.7

## Features

- **Image generation and image editing** can now be toggled independently via [features] in config.toml.

## Bug Fixes

- **Background tasks** started inside subagents now continue running after the subagent session ends.


# 0.2.6

## Bug Fixes

- **Background tasks** started inside subagents now continue running after the subagent session ends.
- **Image description** now reliably uses the grok-build model instead of falling back to the active session model.


# 0.2.5

## Bug Fixes

- **Drag-and-drop** and pasting images or files now works correctly on Windows.


# 0.2.4

## Features

- **image_gen** now uses the higher-quality grok-imagine-image-quality model.

## Bug Fixes

- **read_file** now correctly passes embedded base64 images to the model as vision tokens instead of truncated fragments.


# 0.2.3

## Features

- Memory system: /remember command, note modal with raw/enhanced preview, grow/memory/rewrite ACP extension, Ctrl+F fullscreen toggle for /memory modal.
- Agent configuration: /config-agents modal with definitions and defaults.
- Goal classifier: end-to-end goal tracking with subagent-powered classification.


# 0.2.2


# 0.2.1

## Bug Fixes

- **Pasting or dropping images** now succeeds for truncated, CRC-corrupt, or tiny files instead of failing silently.


# 0.2.0

## Performance

- **Large chat sessions** now use substantially less memory and run faster during forks, rewinds, and compaction.
