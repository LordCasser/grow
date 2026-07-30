# Configuration

Grow reads settings from config files, environment variables, and CLI flags. This page covers the common options.

---

## Precedence

Settings resolve highest-priority first:

1. **CLI flags** (e.g. `--yolo`, `--model`, `--sandbox`)
2. **Environment variables** (e.g. `GROW_API_KEY`, `GROW_MEMORY`)
3. **config.toml** (`~/.grow/config.toml`)
4. **Managed / requirements config** (files your org may deploy, e.g. `managed_config.toml` / `requirements.toml`)
5. **Built-in defaults**

---

## config.toml (main configuration)

Location: `~/.grow/config.toml`. If the file is missing, Grow uses its built-in defaults, so you only need to set the values you want to override.

### General settings

```toml
[cli]
auto_update = true                     # check for updates on launch

[models]
default = "example/model-a"           # provider/model used for new sessions
default_reasoning_effort = "high"     # only used when the model declares this effort

# Defaults applied to every model; a provider model value always wins.
# See "Custom Models" for the per-model overrides and full details.
extra_headers = { "X-Request-Tags" = "team=example,env=prod" }
temperature = 0.7
top_p = 0.95
output_limit = 8192
max_retries = 8
inference_idle_timeout_secs = 600
stream_tool_calls = true

[ui]
simple_mode = true                     # readline-style prompt editing (default); false = vim editing in the prompt
vim_mode = false                       # vim-style scrollback navigation keys (default: false)
max_thoughts_width = 120               # max column width for reasoning display
default_selected_permission = "always_allow_all_sessions" # preselected row on the FIRST approval prompt
remember_tool_approvals = false        # show per-command "Always allow" options on permission prompts;
                                       # grants are remembered per project (default: false); see 22-permissions-and-safety.md
show_thinking_blocks = true            # show agent thinking blocks in the TUI (default: true)
group_tool_verbs = true                # fold runs of read/search/list tool calls and subagent rows
                                       # — and finished thoughts among them — into one row (default: true)
collapsed_edit_blocks = false          # show edits as one-line +N/-M diffstat summaries and merge
                                       # back-to-back same-file edits into one row, expand for the
                                       # diffs (default: false; pager.toml [scrollback.blocks.edit]
                                       # expanded_by_default/line_summary override its fold shape)
page_flip_on_send = true               # pin a just-sent prompt at the top of the viewport so the
                                       # response starts on a fresh page (default: true); set false
                                       # so sending never moves the scroll position
screen_mode = "fullscreen"             # default render mode: "fullscreen" | "minimal"
                                       # (unset → fullscreen); set via /settings → Default screen mode

[features]
lsp_tools = false                      # expose the lsp tool
codebase_indexing = true               # code graph indexing (default: true)
two_pass_compaction = false            # prefire two-pass compaction (default: false, opt-in)
remote_fetch = false                   # disable optional catalog/settings fetches; explicit BYOK
                                       # provider requests are unaffected

[diagnostics]
crash_handler = true                   # local crash capture; Grow has no telemetry exporter

[session]
auto_compact_threshold_percent = 85    # auto-compact at this % of context window (default: 85)
load_envrc = true                      # load .envrc environment variables

[tools]
respect_gitignore = false              # default: false; set true to make every tool skip gitignored files
```

### Local announcements

Grow does not download announcements. The welcome page, Dashboard, and session banner read the
final local `announcements` array and update when the configuration file changes:

```toml
[[announcements]]
id = "team-notice"
title = "Notice"
message = "Local announcement text"
severity = "info"                    # info | warning | critical | promo
dismissible = true
persistent = false
expires_at = "2026-12-31T23:59:59Z" # optional RFC 3339 timestamp

[announcements.cta]
label = "Open docs"
url = "https://example.com/docs"
caption = "Optional"
```

When `announcements` is absent, Grow shows its built-in provider-neutral notice. A configured
non-empty array replaces that notice completely. Set `announcements = []` to disable announcements.
Hidden announcement IDs remain local in `~/.grow/announcements.json`.

#### Input mode

`[ui] simple_mode` controls how you edit text in the **prompt** — the input editor. It has nothing to do with how you move around the scrollback; that's [`vim_mode`](#vim-mode).

| Value | Behavior |
|-------|----------|
| `true` (default) | **Readline editing.** Plain readline-style text entry. |
| `false` | **Vim editing (experimental).** Vim-style modal editing (normal and insert modes). When the prompt is empty it starts in normal mode with focus on the scrollback. |

To switch the prompt to vim-style editing:

```toml
[ui]
simple_mode = false
```

You can also flip it from the settings pane (`/settings` → **Disable vim input mode**); Grow writes your choice to `[ui] simple_mode`. `simple_mode` and `vim_mode` are independent — one governs the prompt editor, the other governs scrollback navigation. See [Keyboard Shortcuts](03-keyboard-shortcuts.md) for the full binding reference.

#### Default selected permission

When the agent asks to run a command (or take some other tool action), the approval menu highlights one row by default. `[ui] default_selected_permission` sets which row that is on the **first** prompt of a session.

| Value | Preselected row |
|-------|-----------------|
| `always_allow_all_sessions` (default) | The "Always allow on all sessions" row. |
| `allow_command_always` | The "Always allow this command" row. |
| `allow_once` | The "Yes" / allow-once row. |
| `reject` | The reject row. |

```toml
[ui]
default_selected_permission = "allow_once"
```

After you answer the first prompt the cursor turns **sticky**: each later prompt preselects whatever you last confirmed (pick "No" once and subsequent prompts start on their reject row), carrying across edit / bash / MCP prompts until you restart. So this setting only picks the starting point.

Values match case-insensitively; an unset or unrecognized value falls back to `always_allow_all_sessions`. The `allow_command_always` row is always scoped to the specific action being approved (command / tool / domain / edit-session), never a global allow-everything — that's what `always_allow_all_sessions` is for. Note the per-command "Always allow" rows only appear when `[ui] remember_tool_approvals = true` (default false). See [22-permissions-and-safety.md](22-permissions-and-safety.md).

You can also override this with `GROW_DEFAULT_SELECTED_PERMISSION`, which is handy for headless or agent test runs that shouldn't mutate `config.toml`. Precedence: env var → `config.toml` → `always_allow_all_sessions`.

#### Vim mode

`[ui] vim_mode` controls whether vim-style bindings are active in the **scrollback** pane. It does not affect the prompt.

| Value | Behavior |
|-------|----------|
| `false` (default) | Bare-letter and `Shift+letter` keys (`j`/`k`, `h`/`l`, `g`/`G`, `y`/`Y`, `o`/`O`, `r`, `x`, `e`/`E`, `H`/`L`, plus `i`) are suppressed in the scrollback: pressing one focuses the prompt and types the character. Arrows, `Tab`, `Space`, `PageUp`/`PageDown`, and every `Ctrl+letter` shortcut still navigate. `Esc` is **not** a scrollback key — it cancels a running turn, and while idle follows the clear / rewind policy (see [Keyboard Shortcuts](03-keyboard-shortcuts.md#escape)). |
| `true` | All vim-style scrollback bindings are active, exactly as listed in [Keyboard Shortcuts](03-keyboard-shortcuts.md). Mid-turn `Esc` is swallowed in this mode (`Ctrl+C` cancels); minimal mode keeps Esc-cancel regardless. |

Toggle it at runtime with `/vim-mode`, or from `/settings` → **Vim scrollback navigation**. Grow writes the change to `[ui] vim_mode` immediately and applies it to every future pager session, including new agents and subagents in the same process. There's no per-session override — `config.toml` is the source of truth on next launch. `vim_mode` is independent of `simple_mode`.

#### Screen mode

`[ui] screen_mode` is the **default render mode** for plain `grow` launches. Set it from `/settings` → **Default screen mode** (restart required) or edit `config.toml` by hand — both write the file. CLI flags (`--minimal` / `--fullscreen`) and slash commands (`/minimal` / `/fullscreen`) are session-scoped and do **not** write this key; after a slash switch, the reverse command returns you for that session only.

| Value | Behavior |
|-------|----------|
| unset | Settings shows **Fullscreen**. There's no sticky preference at startup: legacy `pager.toml` `[terminal] minimal` can still force minimal, and terminals that leak mouse reports (JediTerm/Windows) may auto-open minimal until you set an explicit value. Otherwise the alt-screen policy picks fullscreen vs inline. |
| `"fullscreen"` | Sticky non-minimal. Fullscreen-vs-inline still follows the alt-screen policy (`--no-alt-screen`, `[terminal] alt_screen`, terminal auto-detection). |
| `"minimal"` | Sticky minimal (scrollback-native) mode. |

A CLI flag always wins over the config value for that invocation.

#### Snap prompt to top on send

By default, sending a prompt scrolls it to the top of the viewport so the response starts on a fresh page. Set `[ui] page_flip_on_send = false` (or toggle **Snap prompt to top on send** in `/settings` → Appearance) to leave the scroll position alone when you send. It takes effect on the next send — no restart.

#### Scrolling

Four `[ui]` settings tune mouse-wheel and trackpad scrolling. All apply immediately and are editable from the settings pane (`/settings` → **Scroll speed** / **Scroll input** / **Scroll lines** / **Invert scroll**).

| Key | Values (default) | Behavior |
|-----|------------------|----------|
| `scroll_speed` | `1`–`100` (`50`) | Speed multiplier for wheel and trackpad. `50` = 1.0x, `1` = 0.1x, `100` = 6.0x. |
| `scroll_mode` | `auto` \| `wheel` \| `trackpad` (`auto`) | Wheel-vs-trackpad detection is heuristic (terminal scroll events carry no magnitude); force one when auto-detection misreads your device — e.g. a wheel notch that jumps too far, or a trackpad that feels stepped. |
| `scroll_lines` | `1`–`10` (unset) | Lines per scroll tick, applied to **both** wheel and trackpad. While unset, each terminal's own profile applies (e.g. a conservative 1 line/event under tmux). Committing any value — even `3`, the number the settings pane shows — switches permanently to that explicit override. |
| `invert_scroll` | `false` \| `true` (`false`) | Reverse vertical scroll direction ("natural" scrolling). |

```toml
[ui]
scroll_speed = 50
scroll_mode = "auto"     # auto | wheel | trackpad
invert_scroll = false
# scroll_lines is unset by default: the per-terminal profile stays in charge.
# scroll_lines = 3
```

Each setting also has an environment-variable override, applied on first load only (again, handy for headless / test runs): `GROW_SCROLL_SPEED`, `GROW_SCROLL_MODE`, `GROW_INVERT_SCROLL` (`1`/`true`/`0`/`false`), and `GROW_SCROLL_LINES`. Precedence: env var → `config.toml` → default. Unrecognized values fall back to the default, and out-of-range numbers clamp.

### Tool configuration

```toml
[toolset.bash]
timeout_secs = 120.0                   # foreground command timeout in seconds (default: 120)
output_byte_limit = 20000              # max captured output in bytes (default: 20000)

[toolset.ask_user_question]
timeout_enabled = true                 # false = wait forever for answers (default: true)
timeout_secs = 1800                    # seconds to wait when enabled (default: 1800 / 30 min)

[toolset.web_fetch]
proxy_endpoint = "https://proxy.example.com"   # egress proxy URL
allowed_domains = ["docs.rs", "example.com"]   # override the built-in allowlist
allow_local = false                            # true = allow localhost / 127.0.0.0/8 / ::1 only
```

`allow_local` is off by default (SSRF fail-closed). Turn it on (or set `GROW_WEB_FETCH_ALLOW_LOCAL=1`) and `web_fetch` may reach **explicit** loopback hosts only — private, link-local, and cloud-metadata ranges stay blocked. Resolution: TOML > env > default off.

`[toolset.ask_user_question]` is honored across **requirements.toml**, **managed config**, and your user **`config.toml`**. Precedence: requirements → env (`GROW_ASK_USER_QUESTION_TIMEOUT_ENABLED` / `GROW_ASK_USER_QUESTION_TIMEOUT_SECS`) → user config → managed → defaults. Set `timeout_enabled = false` in your user config to disable the automatic questionnaire timeout for yourself; `timeout_secs` must be a positive integer. You can also toggle `timeout_enabled` from `/settings` → **Ask-Question timeout** (under Agent & Approval); changes apply to newly started sessions.

### Authentication

Credentials are provider-scoped. Prefer an environment variable declared by `env_key`; use
`api_key` only when storing a secret in TOML is acceptable. OAuth and credential helpers are
optional provider mechanisms, not a global Grow login requirement. See
[Authentication](02-authentication.md) for the full story.

```toml
[provider.example.options]
base_url = "https://api.example.com/v1"
env_key = "EXAMPLE_API_KEY"
```

### Custom models

Every selectable model belongs to an explicit provider. The provider chooses the wire backend and
owns shared endpoint/credential options; each model owns its API identifier and local limits.

```toml
[models]
default = "example/model-a"
output_limit = 8192

[provider.example]
api_backend = "responses"             # chat_completions | responses | messages

[provider.example.options]
base_url = "https://api.example.com/v1"
env_key = "EXAMPLE_API_KEY"           # string or ordered array of env-var names
query_params = { api-version = "2026-07-22" }
env_http_headers = { "X-Tenant" = "TENANT_TOKEN" }

[provider.example.models.model-a]
name = "Model A"
context_window = 128000                # local context management / auto-compact
output_limit = 16384                   # overrides [models].output_limit
reasoning_efforts = ["none", "high"]
```

There is no built-in model to override. `output_limit` maps to `max_tokens` for Chat Completions and
Messages, and `max_output_tokens` for Responses. See [Custom Models](11-custom-models.md) and the
repository's [`config.example.toml`](../../../../../config.example.toml) for complete examples.

### MCP servers

Grow does not bundle a Web Search provider. Configure any MCP server that exposes search tools if
you need web search; those tools are discovered like every other MCP tool and may use any server or
tool name. The built-in `web_fetch` tool remains available for fetching a known URL.

Configure external tool integrations over the Model Context Protocol.

```toml
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_xxx" }
enabled = true                        # enable/disable (default: true)
startup_timeout_sec = 30              # init timeout in seconds (default: 30)
tool_timeout_sec = 6000              # tool call timeout in seconds (default: 6000)
tool_timeouts = { create_issue = 120 }  # per-tool timeout overrides

[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]

[mcp_servers.my-streamable-server]
url = "https://mcp.example.com/api/mcp"  # HTTP/SSE transport
headers = { "x-mcp-session-id" = "{{session_id}}" }
```

MCP servers can also be set per-project in `.grow/config.toml`. Project-scoped config contributes `[mcp_servers]`, `[plugins]`, and `[permission]` rules; every other section loads only from `~/.grow/config.toml`.

Priority for `[mcp_servers]` and `[plugins]`: `.grow/config.toml` (current dir) > `<repo-root>/.grow/config.toml` > `~/.grow/config.toml`. `[permission]` rules aren't overridden by priority — they merge across all files with `deny` > `ask` > `allow` (see [22-permissions-and-safety.md](22-permissions-and-safety.md)).

### Memory

Persist knowledge across sessions (requires `--experimental-memory` or `GROW_MEMORY=1`).

```toml
[memory]
enabled = false                       # enable memory

[memory.session]
save_on_end = true                    # write metadata summary on session end

[memory.watcher]
enabled = true                        # watch memory files for external edits

[memory.search]
max_results = 6                       # default number of results
min_score = 0.35                      # minimum relevance score

[memory.initial_injection]
enabled = true                        # auto-inject memory on first turn
min_score = 0.0                       # score threshold for first-turn injection

[memory.embedding]
model = "embedding-model"             # embedding model name
dimensions = 1024                     # vector dimensions
```

### Subagents

```toml
[subagents]
enabled = true

[subagents.toggle]
explore = true                        # enable/disable specific types
plan = false

[subagents.models]
explore = "grow-build"               # route to different models
```

To pin the model a subagent uses, set its entry under `[subagents.models]`.

### Goal mode and background workflows

`/goal` has two drivers, chosen by the background-workflows setting. With workflows enabled, the host-owned workflow engine evaluates rounds and drives completion verification; with them disabled, `/goal` falls back to the legacy model-facing `update_goal` tool. Whether `/goal` is available at all is a separate switch (the goal feature setting).

Background workflows — the `workflow` tool, named `.grow/workflows/*.rhai` scripts, `/deep-research`, and `/workflow` launches — are **on by default**. Disable with config, env, or remote settings.

```toml
[workflows]
enabled = false                       # disable background workflows (or GROW_WORKFLOWS=0)
```

Project workflows are discovered from `<repo-root>/.grow/workflows/`; user workflows from `~/.grow/workflows/`. Discovery and invocation key off the script's `meta.name`, so keep each filename aligned with its `meta.name`. Built-ins win over project names, and project names win over user names, so keep names unique across scopes.

Each launch gets a session-unique display handle such as `deep-research-2`. That handle is what you see in the `/workflows` run dashboard and pass to `/workflow pause`, `resume`, or `stop` — the internal run IDs never surface in commands. A numbered handle isn't a reusable definition name, so the dashboard disables **save** until you pick a new unique `meta.name` and save the edited script yourself. See [Slash Commands](04-slash-commands.md) for examples.

### Skills

```toml
[skills]
paths = ["~/my-team-skills"]          # additional directories to scan
ignore = ["~/my-team-skills/wip"]     # paths to exclude
disabled = ["wip-skill"]              # skill names to keep listed but inactive
```

### Harness compatibility

Control vendor compatibility for Cursor, Claude, and Codex. Every cell defaults to `true`. Session cells stay staged and inert until a foreign-session scanner consumes them, and each tool needs both its `sessions` cell and the matching `resume-claude`, `resume-codex`, or `resume-cursor` skill — a missing skill means zero foreign-session filesystem I/O.

```toml
[compat.cursor]
skills = true     # scan ~/.cursor/skills/ and <cwd>/.cursor/skills/
rules = true      # scan ~/.cursor/rules/ and <dir>/.cursor/rules/
agents = true     # scan ~/.cursor/ for named instruction files
mcps = true       # scan ~/.cursor/mcp.json and <cwd>/.cursor/mcp.json
hooks = true      # scan ~/.cursor/hooks.json and <cwd>/.cursor/hooks.json
sessions = true   # staged; no scanner consumer yet

[compat.claude]
skills = true     # scan ~/.claude/skills/ and <cwd>/.claude/skills/
rules = true      # scan ~/.claude/rules/ and <dir>/.claude/rules/
agents = true     # scan ~/.claude/ and <dir>/.claude/CLAUDE*.md
mcps = true       # scan ~/.claude.json for MCP servers
hooks = true      # scan ~/.claude/settings.json for hooks
sessions = true   # staged; no scanner consumer yet

[compat.codex]
sessions = true   # staged; no scanner consumer yet
```

Codex's `skills`, `rules`, `agents`, `mcps`, and `hooks` cells are reserved and currently inert — they do not enable `.codex` discovery.

For Claude and Cursor, `rules` and `agents` are independent: turning off named instruction files doesn't disable the home or project rules directory, and turning off rules doesn't disable named files. Claude's `agents` cell gates home-level `~/.claude/` named files and project `<dir>/.claude/CLAUDE*.md`; generic top-level `Claude.md`, `CLAUDE.md`, and `CLAUDE.local.md` stay recognized. Project rule paths are scanned at every directory from the repo root down to the current one.

Each cell can be set via environment variable or `config.toml`; see the environment-variables reference for the names. Resolution: env var > config.toml > default (on).

`grow inspect` reports cells that still need session-start resolution as `?` until a value is available; cells with an explicit env or TOML value use that value. Affected discovery entries report `compatibilityStatus: "unresolved"` in JSON and `[compat unresolved]` in human output.

### Plugins

```toml
[plugins]
paths = ["~/my-plugins/custom-tools"]
disabled = ["user/a1b2c3d4/noisy-plugin"]
```

### Hints

`[hints]` holds small persisted UI preferences — mostly "stop asking me" opt-outs. Grow writes these for you when you pick a "don't ask again" option in the TUI, but you can edit or delete them by hand; removing a key restores the default.

`[hints]` is read from the **effective config merge**, with the usual precedence: system managed → user `managed_config.toml` → user `config.toml` → user `requirements.toml` → system `requirements.toml`, higher layers winning. The TUI only ever **writes** opt-outs to your user `~/.grow/config.toml`.

```toml
[hints]
project_picker_disabled = false        # skip the project-directory picker
memory_modal_fullscreen = false        # remember the memory modal fullscreen state
new_session_worktree_mode = "never"    # /new worktree prompt: "ask" | "always" | "never"
fork_worktree_mode = "ask"             # /fork worktree prompt: "ask" | "always" | "never"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `project_picker_disabled` | bool | `false` | When `true`, skips the picker that asks you to choose a project directory on the first prompt when Grow launches from a non-project directory (home, Desktop, Downloads, `/tmp`). Set automatically when you choose **"Don't ask me again"** in that picker. Teams can pin it in `managed_config.toml` or `requirements.toml`. |
| `memory_modal_fullscreen` | bool | `false` | Remembers whether the memory modal was last opened fullscreen. |
| `new_session_worktree_mode` | string | `"never"` | Worktree prompt for `/new`: `ask` shows the popup, `always` creates a worktree, `never` skips it. |
| `fork_worktree_mode` | string | `"ask"` | Worktree prompt for `/fork`: `ask`, `always`, or `never`. |

### Notifications

Fire terminal notifications when the agent finishes a turn or needs approval. They use terminal-native protocols (OSC 9, OSC 99, OSC 777, or BEL) and are focus-gated by default, so they only fire when you're not looking at the terminal.

```toml
[ui.notifications]
method = "auto"           # auto|osc9|osc99|osc777|bel|none
condition = "unfocused"   # unfocused|always|never
idle_threshold_secs = 3   # seconds unfocused before a notification fires
events = ["turn_complete", "approval_required"]
sleep_prevention = true   # prevent display sleep during agent turns
progress_bar = true       # show tab progress bar (OSC 9;4)

[ui.notifications.title]
enabled = true
items = ["action-required", "spinner", "activity", "session-name", "grow"]
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `method` | string | `"auto"` | Notification protocol. `auto` picks the best for your terminal. |
| `condition` | string | `"unfocused"` | When to notify: `unfocused` (only when the terminal lost focus), `always`, or `never`. |
| `idle_threshold_secs` | integer | `3` | Minimum seconds unfocused before a notification fires. |
| `events` | array | `["turn_complete", "approval_required"]` | Events that trigger notifications. Options: `turn_complete`, `approval_required`, `session_ready`, `task_complete`, `agent_error`. |
| `sleep_prevention` | bool | `true` | Keep the display awake while the agent works (macOS/Linux). |
| `progress_bar` | bool | `true` | Show a progress indicator in the terminal tab (OSC 9;4). |
| `title.enabled` | bool | `true` | Set the terminal title to reflect agent state. |
| `title.items` | array | (see above) | Items shown in the title bar. Options: `action-required`, `spinner`, `activity`, `session-name`, `cwd`, `model`, `turn-timer`, `grow`. |

#### Terminal support matrix

| Terminal | Auto Protocol | Focus Tracking | Progress Bar |
|----------|---------------|----------------|--------------|
| iTerm2 | OSC 9 | Yes | Yes |
| Kitty | OSC 99 | Yes | No |
| Ghostty | OSC 777 | Yes | Yes |
| WezTerm | OSC 9 | Yes | Yes |
| Warp | OSC 9 | Yes | No |
| Alacritty | BEL | Yes | No |
| VS Code | BEL | Yes | No |
| Apple Terminal | BEL | No | No |
| VTE (GNOME Terminal) | OSC 777 | Yes | No |
| Unknown | BEL | No | No |

With `method = "auto"`, Grow detects the terminal brand and picks the best protocol. Set `method` explicitly to override that.

#### Notification hooks

Run your own commands when events fire. Hooks receive `$GROW_EVENT`, `$GROW_MESSAGE`, and `$GROW_SESSION_ID` in the environment.

```toml
# macOS native notification
[[ui.notifications.hooks]]
command = "terminal-notifier -title 'Grow' -message '$GROW_MESSAGE'"
events = ["turn_complete", "approval_required"]
only_unfocused = true
timeout_secs = 10

# Push to ntfy server
[[ui.notifications.hooks]]
command = "curl -s -d '$GROW_MESSAGE' ntfy.sh/my-grow-alerts"
events = ["turn_complete"]
only_unfocused = true
timeout_secs = 10

# Play a sound
[[ui.notifications.hooks]]
command = "afplay /System/Library/Sounds/Glass.aiff"
events = ["turn_complete"]
only_unfocused = true
timeout_secs = 5
```

| Hook Option | Type | Default | Description |
|-------------|------|---------|-------------|
| `command` | string | (required) | Shell command to run. |
| `events` | array | `[]` | Events that trigger this hook (empty = all events). |
| `only_unfocused` | bool | `true` | Only fire when the terminal has lost focus. |
| `timeout_secs` | integer | `10` | Kill the hook process after this many seconds. |

#### Troubleshooting

Run `/doctor` in the affected session. It shows the detected notification and focus issues, the relevant configuration file, and the steps to resolve them. An explicit `method = "bel"` is treated as intentional. `method = "none"` turns off notification and focus findings.

**Sleep prevention not taking effect:** on macOS, sleep prevention uses `IOPMAssertionCreateWithName` via CoreFoundation; on Linux, `systemd-inhibit` (which must be on `$PATH`). Make sure the relevant tool is available. Prevention is only active during agent turns and releases automatically when the turn ends.

### Keyboard shortcuts

Keyboard shortcuts are **not** configurable — all bindings are built in. See [Keyboard Shortcuts](03-keyboard-shortcuts.md) for the complete reference.

### Local diagnostics

Grow has no telemetry or remote diagnostics configuration. Use `--debug`,
`GROW_DEBUG_LOG`, `GROW_LOG_FILE`, and `RUST_LOG` to write diagnostics locally.
See [Local Diagnostics](24-monitoring-usage.md).

### Version pinning

Control which versions the CLI may auto-update to and which versions may run. Set
these in `[cli]`, or in a managed layer for fleet-wide policy. Each has an
environment override that can only tighten the bound, for CI and testing.

> **Changed:** `minimum_version` no longer blocks startup. It is now a soft
> anti-downgrade floor for the updater. For a hard floor that prevents old
> versions from starting, use `required_minimum_version`.

```toml
[cli]
minimum_version = "1.0.0"          # updater won't downgrade below this
maximum_version = "1.9.0"          # updater won't install above this
required_minimum_version = "1.0.0" # refuse to start below this
required_maximum_version = "1.9.0" # refuse to start above this
```

- `minimum_version` (`GROW_MINIMUM_VERSION`) is a soft anti-downgrade floor. The
  updater skips a target below it and keeps the current version. It never blocks
  startup.
- `maximum_version` (`GROW_MAXIMUM_VERSION`) is a soft ceiling. The updater caps
  its target at it and never installs above it.
- `required_minimum_version` (`GROW_REQUIRED_MINIMUM_VERSION`) and
  `required_maximum_version` (`GROW_REQUIRED_MAXIMUM_VERSION`) are hard bounds. If
  the running version is outside the range, the CLI exits at startup and instructs
  the user to install an approved version. `grow update` and `grow --version` keep
  working so an out-of-range install can recover.
- Bounds resolve across config layers by tightening only: a floor takes the
  highest value and a ceiling the lowest, so a managed bound can't be loosened,
  and a user or environment bound can't cancel a managed hard bound. An invalid
  value is ignored so a bad policy can't block startup.
- An explicit `grow update --version X` is allowed above the ceiling, to recover
  from a too-new install, and rejected below the hard floor.

### Enterprise deployment

A complete config for enterprise use:

```toml
[cli]
auto_update = false

[models]
default = "company/company-coder"
output_limit = 65536

[auth_provider.company]
type = "command"
command = "/usr/local/bin/my-company-auth-provider"
token_ttl_secs = 3600

[provider.company]
api_backend = "responses"

[provider.company.options]
base_url = "https://llm-proxy.acme.com/v1"
auth_provider = "company"

[provider.company.models.company-coder]
name = "Company Coder"
context_window = 128000
output_limit = 65536
```

---

## pager.toml (appearance configuration)

Location: `~/.grow/pager.toml`. This controls the TUI's look and feel. Changes apply on restart.

### Terminal

```toml
[terminal]
alt_screen = "auto"                   # fullscreen mode: "auto", "always", "never"
```

- `auto` (default): use the alternate screen when the terminal supports it.
- `always`: always use the alternate screen.
- `never`: run inline in the terminal's main scrollback buffer.

### Animation

```toml
[animation]
fps = 30                              # animation frame rate (ticks per second)
wave_rows = 32                        # rows per wave cycle for accent animation
```

### Prompt

```toml
[prompt]
collapse_unfocused = true             # collapse prompt when scrollback is focused
mouse_hover = true                    # show hover highlight on the prompt widget
show_prefix = true                    # show the prompt prefix character
```

Compact mode isn't persisted here — control it at runtime with `[ui] compact_mode` or the `/compact-mode` command.

### Scrollback

```toml
[scrollback.layout]
outer_vpad = 1                        # vertical padding
outer_hpad_left = 2                   # left horizontal padding
outer_hpad_right = 2                  # right horizontal padding
block_pad_left = 2                    # padding inside block, left of content
block_pad_right = 2                   # padding inside block, right of content

[scrollback.scrollbar]
enabled = true                        # show scrollbar
gap_left = 0                          # gap between content and scrollbar
gap_right = 0                         # gap between scrollbar and screen edge

[scrollback.scroll]
margin = 0                            # minimum context lines above/below selection
min_page_fraction = 0                 # minimum scroll as % of viewport (0-100)
follow_indicator = "center"           # follow indicator: "center" or "none"
follow_auto_select = true             # auto-select latest entry in follow mode
follow_by_overscroll = true           # scrolling past bottom engages follow mode
anchor_on_fold = true                 # keep block position when folding
respect_manual_folds = true           # opt-in (default: false): keep manually folded blocks as-is during streaming/finish; expanding while following stops auto-scroll

[scrollback.display]
sticky_headers = true                 # pin user prompts as sticky headers
tab_width = 4                         # spaces per tab character
expandable_indicator = true           # show expand indicator on foldable entries
expandable_indicator_running = true   # show indicator on running entries
expandable_indicator_char = "›"       # character for the expand indicator (default: "›")
selection_buttons = false             # show copy/view buttons on selection
line_under_last_entry = false         # horizontal line below last entry
group_selection_split = true          # split selection box for expanded blocks
highlight_overlays_border = false     # highlight extends over selection box border
dim_accent = 0.5                      # dimming factor for collapsed accents (0.0-1.0)
```

`respect_manual_folds` is off by default. Turn it on and a block you fold by hand is pinned: streaming updates and finish events (a thinking block ending, say) leave its fold state alone, and expanding a block while follow-mode is tailing new content stops the auto-scroll so the view stays put. Follow resumes via `Shift+G`, `j` at the last entry, scrolling past the bottom, or sending a new prompt. `Shift+E` clears all pins; `Ctrl+E` clears pins on thinking blocks.

### Block configuration

```toml
[scrollback.blocks.edit]
indent = true                         # indent diff content
vpad = false                          # vertical padding
# expanded_by_default = true          # unset: follows [ui] collapsed_edit_blocks in config.toml
                                      # (flag on = collapsed one-liner); uncomment to pin either shape
dual_line_numbers = false             # two-column line numbers (old + new)
# line_summary = false                # show +N/-M in the collapsed header; unset follows the same flag
hunk_separator = "…"                  # separator between diff hunks (default: "…")

[scrollback.blocks.prompt]
vpad = true                           # vertical padding
show_prefix = true                    # show prompt prefix character
min_lines = 2                         # minimum content lines in sticky mode

[scrollback.blocks.thinking]
animate = true                        # animated accent while thinking
truncated_lines = 3                   # lines in truncated mode
```

### Todo

```toml
[todo]
badge_format = "default"              # "default", "colon", or "comma"
```

Badge format examples:

- `default`: `2/5` — a `done/total` progress fraction (done = completed, total = all tasks except cancelled).
- `colon`: `[>:1 [ ]:4 ok:3 x:2]` — icon:count.
- `comma`: `[1 >, 4 [ ], 3 ok, 2 x]` — count icon, comma-separated.

### Plugins

```toml
disable_plugins = false               # hide hooks/plugins UI entirely
```

---

## Environment variables

Common variables are listed below.

### Authentication

| Variable | Description |
|----------|-------------|
| `GROW_API_KEY` | Fallback API key for configured providers that do not declare their own `api_key` or `env_key` |
| `GROW_AUTH_PROVIDER_COMMAND` | External auth binary path |
| `GROW_AUTH_PROVIDER_LABEL` | Display name on TUI login screen |
| `GROW_AUTH_TOKEN_TTL` | Token lifetime in seconds |
| `GROW_AUTH_EARLY_INVALIDATION_SECS` | Seconds before expiry to refresh (default: 300) |
| `GROW_OIDC_ISSUER` | OIDC issuer URL |
| `GROW_OIDC_CLIENT_ID` | OIDC client ID |

### Endpoints

| Variable | Description |
|----------|-------------|
| `GROW_CLI_CHAT_PROXY_BASE_URL` | Override API proxy base URL |

### Features

| Variable | Description |
|----------|-------------|
| `GROW_MEMORY` | Enable (`1`) or disable (`0`) cross-session memory |
| `GROW_SUBAGENTS` | Enable (`1`) or disable (`0`) subagents |
| `GROW_WORKFLOWS` | Enable (`1`) or disable (`0`) background workflows and select the `/goal` driver (default on: host-owned workflow driver; off: legacy `update_goal`) |
| `GROW_WEB_FETCH` | Enable (`1`) or disable (`0`) the web_fetch tool |
| `GROW_WEB_FETCH_ALLOW_LOCAL` | Allow `web_fetch` to explicit loopback hosts only (`localhost` / `127.0.0.0/8` / `::1`). Same as `[toolset.web_fetch] allow_local`. Default off; private/metadata stay blocked. |
| `GROW_AGENT` | Custom agent definition path or name |
| `GROW_SANDBOX` | Sandbox profile (off, workspace, devbox, read-only, strict; or a custom profile name) |

### Logging

| Variable | Description |
|----------|-------------|
| `GROW_LOG_FILE` | Write logs to this file path (used verbatim as the path) |
| `RUST_LOG` | Log level filter (e.g. `debug`); controls the `GROW_LOG_FILE` log and headless stderr output |

### Paths

| Variable | Description |
|----------|-------------|
| `GROW_HOME` | Override config directory (default: `~/.grow`) |
| `GROW_ROOT` | Optional multi-user workspace root used together with `GROW_USER` |
| `GROW_USER` | User name or relative user directory under `GROW_ROOT`; bare names resolve below `users/` |
| `GROW_RESPECT_GITIGNORE` | Force gitignore filtering on (`1`) or off (`0`); overrides `[tools] respect_gitignore` |

## File locations

| Path | Description |
|------|-------------|
| `~/.grow/config.toml` | Main configuration file |
| `~/.grow/pager.toml` | TUI appearance configuration |
| `~/.grow/auth.json` | Authentication credentials (auto-managed) |
| `~/.grow/sessions/` | Persisted sessions (organized by working directory) |
| `~/.grow/memory/` | Cross-session memory files and index |
| `~/.grow/skills/` | User-scoped skill definitions |
| `~/.grow/plugins/` | User-scoped plugins |
| `~/.grow/agents/` | User-scoped agent definitions |
| `~/.grow/lsp.json` | LSP server configuration (user-scoped) |
| `~/.grow/logs/` | Internal log files (e.g. `unified.jsonl`, MCP server logs) |
| `.grow/config.toml` | Project-scoped MCP servers, plugins, and permission rules |
| `.grow/skills/` | Project-scoped skill definitions |
| `.grow/plugins/` | Project-scoped plugins |
| `.grow/agents/` | Project-scoped agent definitions |
| `.grow/hooks/` | Project-scoped hooks |
| `.grow/lsp.json` | LSP server configuration |

---

## Project-scoped configuration

Some settings can be set per-project by placing files in `.grow/` inside your repository:

| File | What it configures |
|------|--------------------|
| `.grow/config.toml` | MCP servers, plugins, permission rules, and the `[mcp] max_output_bytes` tool-result cap (other sections load only from `~/.grow/config.toml`) |
| `.grow/skills/` | Project-specific skills |
| `.grow/hooks/` | Project-specific lifecycle hooks |
| `.grow/agents/` | Project-specific agent definitions |
| `.grow/lsp.json` | LSP server configuration |
| `.grow/sandbox.toml` | Custom sandbox profiles |
| `AGENTS.md` | Project instructions (system prompt) |

Project-scoped MCP servers override global ones with the same name (full replacement, not a merge).

---

## LSP servers

Language servers power passive diagnostics and the optional `lsp` tool (see the [`lsp_tools`](#general-settings) feature flag). Definitions come from three sources and merge by server name:

| Source | Location | Scope |
|--------|----------|-------|
| User | `~/.grow/lsp.json` | All projects |
| Project | `.grow/lsp.json` | Current repository |
| Plugin | A trusted plugin's `.lsp.json` file, or an inline `lspServers` block in its `plugin.json` | Wherever the plugin is enabled |

When the same server name comes from more than one source, it resolves highest-priority first:

1. **Project** — `.grow/lsp.json`
2. **User** — `~/.grow/lsp.json`
3. **Plugins** — file-based `.lsp.json`, then inline `lspServers`, in plugin load order

Project and user entries replace lower-priority ones of the same name. Plugin entries only add servers whose names aren't already defined by a local file, so a local `lsp.json` always wins over a plugin. Plugin LSP servers load only after the plugin is trusted (see [Plugins](09-plugins.md)).
