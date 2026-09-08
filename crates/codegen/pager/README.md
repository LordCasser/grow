# pager

Terminal UI (TUI) for Grow. Provides the interactive full-screen interface
including the scrollback view, prompt input, session management, and all modal
dialogs.

## Architecture

```
src/
├── app/                 # Application state and event handling
│   ├── root/            # AppView, Action → Effect dispatch, effects, event loop
│   ├── session/         # AgentSession and read-only activity projection
│   ├── agent_view/      # Per-session presentation owner and domain methods
│   └── acp_handler/     # ACP protocol routing into semantic owner methods
├── views/               # UI components
│   ├── prompt_widget.rs # Text editor with file search, slash, history
│   ├── welcome/         # Welcome screen (logo, menu, prompt)
│   ├── extensions_modal.rs   # Extensions modal (hooks, plugins, marketplace, skills, MCP servers)
│   ├── file_search/     # @-completion dropdown and line viewer
│   ├── slash_dropdown.rs# /command completion dropdown
│   └── ...              # Scrollback, status bar, panes, etc.
├── scrollback/          # Message history rendering
├── slash/               # Slash command registry and built-in commands
├── appearance/          # Theme and pager.toml config
├── acp/                 # Agent Communication Protocol client state
└── render/              # Low-level rendering helpers (color, wrapping, etc.)
```

## Key Concepts

- **AppView** — the root owner for navigation, the agent roster, dashboard,
  session picker, global configuration, the effect mailbox, and root render state
- **AgentSession** — the per-session owner for Shell/ACP facts, including
  foreground and queue state, Behavior/Goal/Workflow projections,
  replay/reconnect cursors, event highwaters, prompt identity, terminal
  finalization, and runtime activity
- **AgentView** — one presentation owner per session; owns the prompt,
  scrollback, panes, overlays, selection, links, media, geometry, and render caches
- **PromptWidget** — text editor component with file search (`@`), slash commands (`/`), history search, and paste elements
- **Action/Effect** — Elm-style architecture: input → Action → dispatch → Effect → state update

`AppView`, `AgentView`, and `AgentSession` are composition roots, not public state
bags. ACP routing and synchronous dispatch update them through semantic methods.
Operations such as permission handling, replay terminal settlement, and workflow
ingest that affect projection, scrollback, and overlays are committed by one
`AgentView` domain method so the visible state cannot become partially updated.

## Dependency and mutation rules

The crate direction is fixed:

```text
pager-minimal -> pager -> pager-render
```

`pager` must not depend on `pager-minimal`, and `pager-render` must not depend on
Pager view types. `pager-minimal` may receive `AppView`/`AgentView` references in
its renderer signature, but all state access goes through `pager::minimal_api`.
The reverse call direction remains the installed function-pointer hook in
`minimal_hook`.

Dispatch remains synchronous and deterministic: `Action -> Effect`. Effects
perform I/O in the event loop and feed results back as actions; reducers do not
await. Render code may update presentation caches and geometry, but must never
mutate runtime/liveness facts or infer the Shell foreground owner. Timeline is
the only persistent fact source, while Pager state is a projection plus
presentation state.

## Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl+P` or `?` | Agent screen | Open command palette |
| `Ctrl+L` | Any (non–VS Code family) | Open plugins/hooks modal; on VS Code / Cursor / Windsurf / Zed use `/plugins` or `/hooks` (`Ctrl+L` is mid-turn interject) |
| `Tab` | Prompt | Switch to scrollback |
| `Esc` | Turn running | Cancel — in minimal mode or with vim scrollback mode off (the default). Fullscreen vim mode: no-op (use `Ctrl+C`) |
| `Esc` `Esc` | Idle, non-empty prompt | Clear prompt (within 800ms; first press shows hint) |
| `Esc` `Esc` | Idle, empty prompt + messages | Open rewind picker (silent first press) |
| `Ctrl+M` | Prompt | Toggle multiline mode |
| `Shift+Enter` | Prompt | Insert newline |
| `/` | Prompt | Start slash command |
| `@` | Prompt | Start file search |
| `!` | Prompt (empty) | Enter bash mode |
| `Ctrl+C` | Prompt (with text) | Clear prompt (even while turn running) |
| `Ctrl+C` | Prompt (empty) + turn running | Cancel running turn |
| `Ctrl+B` | Agent screen + foreground command running | Send the command to the background |
| `Ctrl+G` | Agent screen (full TUI) | Toggle the tasks pane |
| `Ctrl+G` | Ordinary composer (minimal mode) | Edit the draft externally; use the command-palette entry if the chord is reserved |

## Docs

- [Terminal Support & Troubleshooting](docs/user-guide/21-terminal-support.md) — tmux/SSH truecolor, clipboard, mouse, diagnostics, `/doctor`
- [Hooks & Plugins Guide](docs/hooks-and-plugins.md) — managing hooks, plugins, and marketplace sources
- [Custom Hooks Guide](docs/custom-hooks.md) — creating, configuring, and writing your own hooks
- [Hook Examples](../hooks/examples/README.md) — sample hooks for common workflows
- [Hooks Crate (`hooks`)](../hooks/) — hook runtime, event types, and execution engine
- [Plugin Marketplace Crate (`plugin-marketplace`)](../plugin-marketplace/) — marketplace source loading, scanning, and install

Skills discovery events request a skill reload, which advertises commands after reconciliation. The watcher does not additionally advertise on directory creation. Workflow-only events retain direct command refresh.

Screen-mode save failures restore the original unset state, so selecting Fullscreen again retries persistence. A displayed default is not treated as an explicitly saved value.

Ordinary setting writes, including the default permission mode, run one at a time per key, retaining only the latest queued choice. Earlier failures leave that choice visible; consecutive failures restore the last confirmed state. Recursive settings actions share the same admission boundary. Default permission writes never notify or mutate the active session; session permission changes use their separate notification path.

CLI `grow export --clipboard` 根据实际剪贴板结果退出和反馈：失败返回错误，未确认发送保留后端提示，不将发送当成到达。交互式 `/export` 的文件回退逻辑独立保留。

交互式 `/export` 的相对文件路径以活动会话 cwd 为基准，绝对路径和展开后的 `~` 路径保留自身目标；CLI `grow export` 的相对路径仍以调用进程目录为基准。

复制及导出反馈中的字符数按 Unicode 标量计算，包含换行；不再把中文/emoji 的 UTF-8 字节数显示为字符数。行数沿用文本行统计，字符数不代表终端列宽。

CLI/TUI 会话文件导出先写同目录临时文件并同步，再原子替换目标，失败不会先截断旧导出。现有文件权限保留；Unix 新文件默认私有权限。已有符号链接保留并更新目标，悬空链接和非普通文件报错。

交互式 `/export` 在会话历史回放期间只提示稍后重试，不导出当前部分 scrollback；加载完成后恢复导出。

`/copy [N] [file]` 的显式相对目标以活动会话 cwd 为基准，展开后的绝对路径保持原样。复制文件仍使用 Unix 0600 私有权限，与 `/export` 的旧权限保留策略不同。

显式复制文件和默认备份先写同目录私有临时文件，完成并同步后替换目标。Unix 新内容提交为 0600；失败保留旧内容和权限。现存符号链接保留并更新目标，悬空链接和非普通目标拒绝。

`/copy N` 只格式化选中的 assistant 消息，找到目标后停止扫描；不会为复制最新回复创建全部历史正文副本。内部 Action 的零或越界索引返回提示。

`/copy` 与 `/export` 的持久通知保留后端实际反馈，未确认的 OSC 52 发送不会称为已到达剪贴板；可用备份文件位置仍显示。

`/copy` 的负数、零和超范围整数参数会报错，不回退成文件名；需要纯数字文件名时使用 `./123` 等显式路径。普通带扩展名路径和合法序号保持不变。

`/copy` 与 `/export` 的显式文件输出通过后台串行任务提交，UI 不再直接执行文件写入和同步。同一应用最多一个写入执行、八个等待，满载时明确提示；请求使用提交时的内容与目标快照，结果只通知原会话。正文渲染和剪贴板/默认备份仍同步，关闭进程不会持久化待处理队列。

`/transcript` 的 Markdown 与 minimal ANSI 快照使用私有临时文件（Unix 0600），由待处理请求持有；替换请求、分页器结束或非重试错误都会释放并尝试清理。挂起超时保留同一文件供重试。正常应用释放也会清理，进程崩溃或强制终止不保证执行清理。契约见 [client-surfaces](../../../../openspec/specs/client-surfaces/spec.md)。

外部分页器无法启动或失败退出时，终端恢复后会显示原因。请求及重试保留原 root/child 会话身份；通知不会写入后来切换到的其他会话正文。原会话可见时 minimal 使用正文通知，其他模式使用 toast；原会话不可见或已移除时，minimal 另在终端显示独立通知行，其他模式显示 toast。成功退出保持静默，挂起超时继续等待重试。

`$PAGER` 支持引号及反斜杠转义的参数边界，例如 `"/Applications/My Pager" --title "Session transcript"`。命令直接启动，不展开变量、命令替换或管道；不完整引号和空程序名会报错。ANSI 快照仍只为 less 补充既有 `-R` 与 `+G`。

minimal 的 transcript 分帧构建在原会话重连时等待，重连完成后自动从最终正文重建，避免输出暂存期间跳过条目形成的前缀。完整回放、增量恢复及失败回滚使用同一规则；普通标签切换仍保留原构建。

首次恢复会话且历史仍在加载时，`/transcript` 会提示加载完成后重试，避免把部分历史当成完整正文打开。minimal 的重连窗口继续保留请求并自动重建。

输入诊断导出使用 `input-debug-<timestamp>-<random>.json`，同秒多次导出各自保留。文件从创建起为 Unix 0600，完整写入并同步后才报告成功，提交前错误会清理新临时文件；原有按键脱敏规则不变。

默认滚动记录文件使用 `scroll-log-<timestamp>-<uuid>.jsonl`，避免同秒记录器相互覆盖。生成路径与启用记录器仍不创建文件，第一条记录到来才打开；显式 `GROW_SCROLL_LOG` 路径保持原行为。

滚动日志打开或写入失败后，`/debug` 状态显示关闭；下一次 `/debug log` 会直接尝试新的默认记录器。待首次写入的记录器仍显示开启，失败不改变滚动计算。

`/debug fps` 与 `GROW_FPS` 也覆盖 minimal：对现有 draw hook 计时，在 live viewport 顶部预留两行读数，正文至少保留三行，空间不足时隐藏读数。关闭后 viewport 恢复原内容高度。统计是同步渲染成本的倒数，不是后台 PTY 完成时间或屏幕刷新率；不增加空闲刷新计时器。

macOS 图片提示的元数据探测遇到原生粘贴读取占用锁时会跳过，不在 UI 等待后台读图。未知分类保留重试机会，确认无图片时按分类自身版本去重。节流和成功显示后的冷却规则保持；这不代表 AppKit 初始化或原生调用具有总超时。契约见 [client-surfaces](../../../../openspec/specs/client-surfaces/spec.md)。

剪贴板类型快照会在分类前后核对版本；探测期间版本变化或类型读取不可用时返回未知，不把它缓存为“没有图片”。返回后外部应用仍可能继续复制，后续粘贴仍使用既有检查路径。

macOS AppleScript 读图回退为每次调用创建独立的 0700 临时目录，目录持有到脚本和读取结束，返回或错误时清理残留，避免并发粘贴共享文件。`GROW_CLIPBOARD_NO_NATIVE_READ` 按环境变量是否存在禁用原生读图（包括值为 `0`）；它保留 AppleScript 回退，不关闭图片粘贴。

macOS 图片脚本通过 osascript 参数接收路径；文件名中的引号、反斜杠、换行和 Unicode 不参与脚本文本解析。该规则覆盖图片读取、附件读取与图片写入。

TUI `/doctor`、修复列表和修复前报告采集在后台执行，每个应用同时一个采集；重复请求提示等待。结果绑定请求时的会话身份，纯报告不会修改配置，具体修复仍显示预览并等待确认。独立 CLI `doctor` 继续同步输出。

共享 tmux 诊断查询对 stdout/stderr 各限制 64 KiB。任一流超限会报告错误并结束进程树，不把截断结果当成有效配置；原有进程期限及退出后清理窗口继续生效。

SSH doctor 修复的 shell 配置目录选择见 [client-surfaces](../../../openspec/specs/client-surfaces/spec.md#requirement-doctor-ssh-fixes-honor-shell-config-directories)：读取进程可见的 ZDOTDIR/XDG_CONFIG_HOME，并将解析后的目标固定在预览和修复计划中。

独立 doctor 的 SSH 已配置状态复用修复目标解析，见 [client-surfaces](../../../openspec/specs/client-surfaces/spec.md#requirement-standalone-doctor-checks-the-selected-shell-target)。
