# MCP Servers

MCP (Model Context Protocol) servers extend Grow with external tool integrations. They let Grow interact with any service that implements the MCP standard.

---

## What Are MCP Servers?

An MCP server is a process that exposes tools to Grow over a standardized protocol. When you configure an MCP server, its tools become available to the model alongside Grow's built-in tools. The model can discover and call these tools during a session.

For example, a GitHub MCP server might expose tools like `create_issue`, `list_pull_requests`, and `search_code`. A database server might expose `query`, `list_tables`, and `describe_schema`.

See the [MCP specification](https://modelcontextprotocol.io) for protocol details.

---

## Configuration

MCP servers are configured in `~/.grow/config.toml` under `[mcp_servers.<name>]` sections.

To distribute MCP servers to a team, or to restrict which servers users may run, see [Distribute across an organization](09-plugins.md#distribute-across-an-organization) in the Plugins guide.

### stdio Transport (Local Process)

Grow spawns a local process and communicates over stdin/stdout:

```toml
[mcp_servers.my-server]
command = "/path/to/server"           # Server executable
args = ["--flag", "value"]            # Command arguments
env = { API_KEY = "sk-..." }          # Environment variables
enabled = true                        # Enable or disable the server (default: true)
tool_scope = "write"                  # "read" only when every tool is side-effect free
startup_timeout_sec = 30              # Server startup timeout, seconds (default: 30)
tool_timeout_sec = 6000               # Per-tool-call timeout fallback, seconds (default: 6000)
tool_timeouts = { slow_op = 120 }     # Per-tool timeout overrides, seconds
```

`tool_scope` is server-wide and defaults conservatively to `"write"`. Set it
to `"read"` only when every tool exposed by that server is side-effect free;
Plan mode uses this same classification before execution approval.

> **Global startup-timeout override:** instead of setting `startup_timeout_sec`
> per server, you can change the default for all servers via the `MCP_TIMEOUT`
> environment variable (milliseconds, compatible with Claude Code) or
> `GROW_MCP_STARTUP_TIMEOUT_SECS` (seconds). A per-server `startup_timeout_sec`
> still takes precedence over both. Cold-start `npx`/`uvx` servers that download
> packages on first launch often need this; the default is 30s.
>
> **MCP tool-result size cap:** large MCP / `use_tool` results are truncated
> inline (full payload spilled under the session `mcp/` folder). Default is
> **20_000 bytes**. Override via:
>
> - env `GROW_MAX_MCP_OUTPUT_BYTES` or `MAX_MCP_OUTPUT_BYTES` (bytes; Grow-native
>   wins if both set; Claude-style name, but we bound by **bytes** not tokens)
> - `config.toml` — user-level (`~/.grow/config.toml`) **or repo-level**
>   (`.grow/config.toml` anywhere on the cwd → git-root chain; the deepest
>   file wins, and the repo value applies only once the folder is trusted):
>
> ```toml
> [mcp]
> max_output_bytes = 40000
> ```
>
> Precedence: requirements.toml > env > repo `.grow/config.toml` >
> user/managed config > default. Repo edits apply to running sessions in that
> directory via config hot-reload.

### HTTP/SSE Transport (Remote Server)

For remote MCP servers accessible over HTTP:

```toml
[mcp_servers.remote-api]
url = "https://mcp.example.com/api"
headers = { "Authorization" = "Bearer token" }
```

### Streamable HTTP with Session ID

```toml
[mcp_servers.my-streamable-server]
url = "https://mcp.example.com/api/mcp"
headers = { "x-mcp-session-id" = "{{session_id}}" }
```

---

## CLI Management

Manage MCP servers from the command line without editing config files:

```bash
# List configured MCP servers
grow mcp list
grow mcp list --json          # Machine-readable output

# Add a stdio server. Everything after -- is the server command, so flags
# like -y reach the server instead of being parsed by grow.
grow mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem /path/to/dir

# Add a stdio server with environment variables (-e is repeatable)
grow mcp add postgres -e DATABASE_URL=postgres://localhost/mydb -- npx -y @modelcontextprotocol/server-postgres

# Add a remote HTTP server
grow mcp add --transport http sentry https://mcp.sentry.dev/mcp

# Add a remote server with an authentication header (--header is repeatable)
grow mcp add --transport http api https://mcp.example.com/mcp --header "Authorization: Bearer YOUR_TOKEN"

# Add a remote SSE server
grow mcp add --transport sse linear https://mcp.linear.app/sse

# Remove a server
grow mcp remove github

# Enable or disable a local/TOML (or compat-sourced) server
grow mcp enable github
grow mcp disable github

# Diagnose a server's configuration and connectivity
grow mcp doctor               # Check every configured server
grow mcp doctor github        # Check one server
grow mcp doctor --json        # Machine-readable output
```

The transport defaults to `stdio`; pass `--transport http` or `--transport sse` for remote servers.

By default `grow mcp add` writes to `~/.grow/config.toml` (`--scope user`). Use `--scope project` to write to `.grow/config.toml` in the current directory instead, which can be committed and shared with your team (see [Project-Scoped MCP Servers](#project-scoped-mcp-servers)). Header and environment variable values are stored verbatim, so reference secrets as `${VAR}` instead of pasting them into a committed project config (see [Example Configurations](#example-configurations)). `grow mcp list` shows servers from both scopes, marking project-scoped ones with `(project)` and disabled ones with `(disabled)`.

`grow mcp remove` searches both scopes and exits 0 after removing the server. It exits 1 when the name is not found, or when the name is defined in both user and project scope — pass `--scope` to say which one to remove.

`grow mcp enable` / `disable` persist the personal on/off state to user `~/.grow/config.toml` (`disabled_mcp_servers`, and `[mcp_servers.<name>].enabled` when that entry exists). Scope:

- **Known names:** user/project Grow TOML, names already on the disabled list, compat sources (`.mcp.json`, Claude, Cursor), **plugin** MCP servers (same discovery as doctor/`/mcps`), and legacy managed `grow_com_*` (no local entry required).
- **Enable only:** if the cwd-nearest project definition has sticky `enabled = false`, that single key is cleared (comments preserved); disable never rewrites project configs.
- **Not full `/mcps` parity:** gateway connectors (`managed_gateway:…`, stored under `disabled_mcp_tools.__managed_gateway_connectors`) stay Space-only in the TUI. Idempotent; unknown names exit 1.

Breaking changes from earlier releases: `--env` now takes one `KEY=value` per flag (use `-e A=1 -e B=2`, not `--env A=1 B=2`), and server names may only contain letters, numbers, hyphens, and underscores.

---

## Project-Scoped MCP Servers

MCP servers can be configured per-project by placing a `.grow/config.toml` in your repository:

```
my-project/
  .grow/
    config.toml
  src/
  ...
```

```toml
# .grow/config.toml
[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
enabled = true
```

When a server exposes a native HTTP/SSE endpoint, prefer the `url` form over wrapping it in a stdio proxy such as `npx mcp-remote <url>`. The native form avoids an extra subprocess per session. Configure any required credentials explicitly as headers or through `bearer_token_env_var`.

Grow walks from the current directory up to the git repo root, loading `.grow/config.toml` at each level:

| Location | Scope | Priority |
|----------|-------|----------|
| `~/.grow/config.toml` | All projects | Lowest |
| `<repo-root>/.grow/config.toml` | This repository | Medium |
| `<cwd>/.grow/config.toml` | Current directory | Highest |

If a project defines a server with the same name as a global one, the project version replaces it entirely (fields are not merged).

Project-scoped files contribute `[mcp_servers]`, `[plugins]`, and `[permission]` entries. Grow reads most other config sections only from `~/.grow/config.toml`.

---

## Tool Naming

MCP tools are namespaced with the server name to avoid collisions:

- Server `filesystem` with tool `read_file` becomes `filesystem__read_file`
- Server `github` with tool `create_issue` becomes `github__create_issue`

---

## Toggle Servers at Runtime

You can enable or disable MCP servers without restarting Grow (TUI `/mcps` or CLI — see [CLI Management](#cli-management)).

### The /mcps Modal

Open the MCP servers modal in the TUI:

- Run `/mcps` as a slash command
- Or press `Ctrl+L` (non–VS Code family) and navigate to the MCP Servers tab; on VS Code family use `/plugins` or `/mcp` and open the MCP Servers tab

From the modal you can:

- See each server's source, enabled state, and tool count
- Enable or disable a server with `Space`
- Expand a server to view the tools it provides
- Refresh the list with `r` after you edit `config.toml`
- Add a server with `a`, or remove a local server with `x` (the modal asks for confirmation; press lowercase `y` to remove, or any other key to cancel)

### Tool Discovery

The model has access to two built-in tools for working with MCP servers:

- `search_tool` — Discover available integration tools across all enabled MCP servers. Use this to find tools by name or description.
- `use_tool` — Call an integration tool discovered via `search_tool`. Specify the fully-qualified tool name (e.g., `github__create_issue`).

---

## Compatibility

Grow loads MCP server configurations from multiple sources for compatibility:

| Source | Format | Location | Configurable |
|--------|--------|----------|-------------|
| `config.toml` | Native Grow config | `~/.grow/config.toml`, `.grow/config.toml` | Always on |
| `.claude.json` | Claude Code format | `~/.claude.json` | `[compat.claude] mcps` |
| `.cursor/mcp.json` | Cursor format | `~/.cursor/mcp.json`, `<project>/.cursor/mcp.json` | `[compat.cursor] mcps` |
| `.mcp.json` | MCP standard format | Project root (cwd to git root) | Loaded unless you have imported or dismissed the Claude import prompt (the import marker is set) |

All sources are merged in priority order: config.toml > Claude > Cursor > `.mcp.json`. Servers from higher-priority sources take precedence when names conflict.

The Claude and Cursor MCP sources are scanned by default. To disable scanning for a specific vendor, set `[compat.<vendor>] mcps = false` in `~/.grow/config.toml` or the corresponding environment variable (`GROW_CURSOR_MCPS_ENABLED`, `GROW_CLAUDE_MCPS_ENABLED`). See [Configuration](05-configuration.md#harness-compatibility) for details. Use `grow inspect` to see which MCP servers were loaded and their vendor origin (`[cursor]`, `[claude]`).

---

## Example Configurations

Use the `url` form for hosted MCP servers and the `command` / `args` form for local stdio tools.

### Native HTTP with BYOK

For hosted, internal, or self-hosted servers, set the required authorization header explicitly:

```toml
[mcp_servers.internal-tools]
url = "https://mcp.internal.example.com/mcp"
enabled = true

[mcp_servers.internal-tools.headers]
Authorization = "Bearer <token>"
```

To avoid putting secrets in the config file, reference an environment variable with `${VAR}` (or `${VAR:-default}`). Grow expands string fields in `[mcp_servers.*]` — `url`, `command`, `args`, and the values in `env` and `headers` — at load time:

```toml
[mcp_servers.internal-tools]
url = "https://mcp.internal.example.com/mcp"
enabled = true
headers = { "Authorization" = "Bearer ${INTERNAL_MCP_TOKEN}" }
```

### Local stdio

Use stdio for tools that must run locally (filesystem access, local databases, in-house servers).

```toml
# Filesystem access scoped to a directory
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/directory"]

# Local Postgres
[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]

# Custom server with a longer startup timeout and tuned per-tool timeouts
[mcp_servers.my-tools]
command = "/usr/local/bin/my-mcp-server"
args = ["--config", "/etc/my-mcp.json"]
startup_timeout_sec = 30
tool_timeout_sec = 120
tool_timeouts = { slow_analysis = 300, quick_lookup = 10 }
```

On Windows, npm installs launchers like `npx`, `npm`, `pnpm`, and `yarn` as `.cmd` batch shims (there is no `npx.exe`). Grow resolves a bare `command` such as `npx` to its real launcher path on `PATH` (honoring `PATHEXT`) before spawning, so these work without manually wrapping them in `cmd /c`. A `command` given as an absolute path or one containing a path separator is used as-is.

---

## Available MCP Servers

A partial list of MCP servers you can configure with the `url` or `command` forms shown above. Confirm the current endpoint or package name with each provider before use:

| Server | Transport | Endpoint / Package |
|--------|-----------|--------------------|
| Filesystem | stdio | `@modelcontextprotocol/server-filesystem` |
| Git | stdio | `@modelcontextprotocol/server-git` |
| GitHub | stdio | `@modelcontextprotocol/server-github` |
| GitLab | stdio | `@modelcontextprotocol/server-gitlab` |
| PostgreSQL | stdio | `@modelcontextprotocol/server-postgres` |
| SQLite | stdio | `@modelcontextprotocol/server-sqlite` |
| Puppeteer | stdio | `@modelcontextprotocol/server-puppeteer` |

See the [MCP Server Registry](https://github.com/modelcontextprotocol/servers) for the full list of community servers and the [MCP specification](https://modelcontextprotocol.io) for protocol details.

---

## Subagents and MCP

Subagents inherit the parent session’s enabled, connected MCP server catalog by default, including plugin-sourced agents. Use agent frontmatter `mcpInheritance` to restrict that hard-eligible set (`all`, `none`, `named`, or `except`). Inheritance is not a runtime grant: `search_tool` marks eligible servers as `granted` or `requires_grant`, and `request_tool_access` grants one server for the live child before `use_tool`. The concrete MCP call still passes through permissions. Details are in [Subagents — MCP inheritance](16-subagents.md#mcp-inheritance).

If a child lists `search_tool` / `use_tool` but returns an empty catalog, check that:

1. The parent session actually connected the server (see Extensions / `grow inspect`)
2. The agent’s `mcpInheritance` is not `none` or a filter that excludes the server
3. Plugin agents cannot declare their own `mcpServers` in frontmatter — they only see parent-connected servers
4. The `search_tool` result's `access` field is `granted`; otherwise request that `mcp_server` first

---

## Troubleshooting

### Server Not Starting

```bash
# Test the server command manually
npx -y @modelcontextprotocol/server-filesystem /path

# Increase startup timeout
# In config.toml:
[mcp_servers.filesystem]
startup_timeout_sec = 30
```

For stdio servers, Grow captures the process's standard error to `~/.grow/logs/mcp/<server>.stderr.log`, truncated on each launch. Check this file when a server starts but fails to handshake:

```bash
tail -f ~/.grow/logs/mcp/filesystem.stderr.log
```

### Viewing Server Status

Use `grow inspect` to see all loaded MCP servers and their sources:

```bash
grow inspect          # Human-readable
grow inspect --json   # Machine-readable
```

### Debug Logging

```bash
RUST_LOG=debug GROW_LOG_FILE=/tmp/grow.log grow
tail -f /tmp/grow.log
```

Look for log entries containing `mcp` to trace server startup, tool discovery, and tool call execution.
