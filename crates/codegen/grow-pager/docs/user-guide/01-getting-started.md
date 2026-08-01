# Getting Started

Grow is a BYOK terminal AI coding assistant forked from xAI Grok Build. It runs as a TUI (Terminal User Interface) that understands your codebase, executes shell commands, edits files, and manages tasks. Web search can be added through a user-configured MCP server.

You can use it interactively as a full-screen TUI, run it headlessly for scripting and CI/CD, or integrate it into editors via the Agent Client Protocol (ACP).

---

## Build from source

Install Rust, DotSlash, `protoc`, and `ripgrep`, then build the composition root. Source builds use
`rg` from `PATH`; official GitHub Release binaries embed it:

```bash
cargo build -p grow-pager-bin --release
./target/release/grow --version
```

---

## First Launch

Start Grow by running:

```bash
grow
```

Before the first connection, configure a provider/model and `[models].default` in
`~/.grow/config.toml`. Grow stops with an actionable prompt when no LLM is configured; it does not
fall back to a bundled model. See [Provider Authentication](02-authentication.md) and
[LLM Providers and BYOK](11-custom-models.md).

---

## Basic Interaction

Once configured, Grow presents a full-screen TUI with two main areas:

- **Scrollback** -- the conversation history showing your prompts, Grow's responses, tool calls, file edits, and more.
- **Prompt** -- the input area at the bottom where you type messages.

Type a message and press `Enter` to send it. Grow reads files, runs commands, and edits code as needed. Each tool run streams into the scrollback in real time.

Press `Tab` to move focus between the prompt and the scrollback. While a turn is running, `Esc` cancels it (the exception is fullscreen vim scrollback mode, where mid-turn `Esc` is a no-op; minimal mode cancels even with vim on); `Ctrl+C` cancels once the composer is empty — with a draft, the first press only clears it. Idle, press `Esc` twice within 800ms to clear a non-empty prompt, or (with an empty prompt and conversation messages) to open rewind — see [Keyboard Shortcuts](03-keyboard-shortcuts.md#escape). With the scrollback focused, use the arrow keys to select entries and to collapse or expand them. To navigate with `j`/`k` and fold with `h`/`l` instead, enable Vim mode.

### File References

Use `@` in your prompt to attach files:

```
@src/main.rs              # Attach a file
@src/main.rs:10-50        # Attach lines 10-50
@src/                     # Browse a directory
```

The `@` operator opens a fuzzy file picker. By default it respects `.gitignore` and hides dotfiles. Prefix with `!` to search hidden files:

```
@!.github                 # Search hidden files
@!.env                    # Attach a .env file
```

### Permissions

By default, Grow asks for permission before executing shell commands or editing files. You can approve individually or select another session policy:

- Press `Ctrl+X`, then `P`, to open the Permission picker
- Use `/permission`, `/ask`, `/auto`, or `/always-approve`
- Use the `--yolo` flag at launch: `grow --yolo`

---

## Key Concepts

### Sessions

Every conversation is a **session**. Sessions are automatically saved to `~/.grow/sessions/` and can be resumed later. Each session tracks the full conversation history, tool calls, file edits, and task state.

- Start a new session: `Ctrl+N` or `/new`
- Resume a previous session: `/resume` in the TUI, or `--resume <ID>` from the CLI
- Continue the most recent session: `grow -c`

### Scrollback

The scrollback is the main display area. It shows:

- **User prompts** -- your messages, rendered as sticky headers
- **Agent messages** -- Grow's responses with full markdown rendering and syntax highlighting
- **Thinking blocks** -- Grow's reasoning process (collapsible)
- **Tool calls** -- file edits (with inline diffs), command executions, search results, and more
- **Task lists** -- TODO items tracking progress

Collapse or expand the selected entry with the `Left`/`Right` arrow keys (or `h`/`l` and `e` in Vim mode). In Vim mode, press `y` to copy its content and `Y` to copy its metadata (for example, the command that ran). Press `Enter` to open it in the fullscreen viewer (in any mode).

### Tools

Grow has built-in tools for:

| Tool | Description |
|------|-------------|
| `read_file` / `search_replace` | Read and edit files with line-precise changes |
| `grep` | Regex search across your codebase (powered by ripgrep) |
| `list_dir` | List directory contents |
| `run_terminal_command` | Execute shell commands |
| `web_fetch` | Fetch a known URL |
| `todo_write` | Create and manage task lists |
| `spawn_subagent` | Spawn parallel subagent sessions |
| `memory_search` | Search cross-session memory |

Tools can be extended with [MCP servers](05-configuration.md#mcp-servers) for integrations like
GitHub, databases, and Web Search. Grow does not require a fixed name for an MCP search tool.

### Slash Commands

Type `/` in the prompt to access commands. These provide quick actions without writing a full prompt:

```
/model provider/model             # Switch to a configured model
/compact                          # Compress conversation history
/always-approve                   # Select Always Approve for this session
/new                              # Start a new session
```

See [Slash Commands](04-slash-commands.md) for the complete reference.

---

## Common Launch Options

```bash
# Launch the interactive TUI and submit an initial prompt as the first turn
grow "fix the failing auth test and run it"

# Initial prompt in a new git worktree. Use --worktree=<name> (with `=`) so the
# prompt isn't swallowed as the worktree name — `grow -w "refactor module X"`
# would treat "refactor module X" as the worktree label, not the prompt.
grow --worktree=feat "refactor module X"

# Base the worktree on a specific branch (e.g. main) instead of the current HEAD:
grow -w --ref main "implement feature from main"


# Start in a specific project directory
grow --cwd ~/projects/my-app

# Add project-specific rules
grow --rules "Always use TypeScript. Prefer functional components."

# Auto-approve all tool executions
grow --yolo

# Use a specific model
grow -m grow-build

# Resume a previous session
grow --resume <session-id>

# Continue the most recent session
grow -c

# Experimental scrollback-native render mode. Sticky: plain `grow` reopens in
# the mode last chosen via --minimal/--fullscreen (or /minimal//fullscreen).
grow --minimal

# Back to the standard fullscreen TUI (and make it sticky again)
grow --fullscreen

# Headless mode (for scripts)
grow -p "Explain this codebase"
```

---

## Headless Mode

Run Grow non-interactively for scripting, CI/CD, and automation:

```bash
grow -p "Your prompt here"
```

Output formats:

| Format | Flag | Description |
|--------|------|-------------|
| `plain` | (default) | Human-readable text |
| `json` | `--output-format json` | Single JSON object with `text`, `stopReason`, `sessionId`, and `requestId` |
| `streaming-json` | `--output-format streaming-json` | NDJSON event stream for real-time processing |

Example CI/CD usage:

```bash
grow -p "Review changes for bugs" --output-format json --yolo | jq -r '.text'
```

---

## Project Rules (AGENTS.md)

Add per-project instructions by creating an `AGENTS.md` file in your repository. Grow reads these files and injects their contents as a project-instructions message at the start of the conversation:

```
~/.grow/AGENTS.md           # Global rules (apply to all projects)
<repo-root>/AGENTS.md       # Repository-level rules
<cwd>/AGENTS.md             # Directory-level rules (highest priority)
```

Deeper files take precedence. Grow also reads `CLAUDE.md` files for compatibility.

---

## Where to Go Next

| Document | What You Will Learn |
|----------|-------------------|
| [Authentication](02-authentication.md) | Provider-scoped API keys, environment variables, and local key helpers |
| [Keyboard Shortcuts](03-keyboard-shortcuts.md) | Complete reference for all key bindings |
| [Slash Commands](04-slash-commands.md) | All available `/` commands |
| [Configuration](05-configuration.md) | config.toml, pager.toml, environment variables |
