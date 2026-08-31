# Hooks

Hooks let you run a script or send an HTTP request at key moments in a Grow session. Use them to automate tasks, enforce safety checks, log activity, send notifications, and integrate your own tools.

---

## What Are Hooks?

A hook is a shell command or HTTP endpoint that Grow calls when a specific lifecycle event occurs. Hooks can:

- **Block actions** -- A `pre_tool_use` hook can deny a dangerous command before it runs.
- **Keep the agent working** -- A `stop` hook can block the agent from finishing its turn until a condition holds (e.g. the test suite passes) and feed the reason back to the model.
- **React to events** -- A `post_tool_use` hook can log every tool execution to a file.
- **Set up context** -- A `session_start` hook can export environment variables or run setup scripts.

---

## Common Use Cases

- **Safety guards**: Block commands such as `rm -rf /` before they run.
- **Audit logging**: Record tool use and sessions to a file or external service.
- **Notifications**: Send a message when a task finishes.
- **Auto-formatting**: Run `cargo fmt` or `prettier` after edits.
- **Environment setup**: Export variables at session start.
- **Custom workflows**: Trigger builds, tests, or deployments on specific events.

---

## Quick Start

1. Create the hooks directory:

   ```sh
   mkdir -p ~/.grow/hooks
   ```

2. Create a hook file, e.g. `~/.grow/hooks/session-start.json`:

   ```json
   {
     "hooks": {
       "session_start": [
         {
           "hooks": [
             { "type": "command", "command": "echo 'Grow session started in '$(pwd)" }
           ]
         }
       ]
     }
   }
   ```

3. Start (or restart) a Grow session. The hook runs automatically on `session_start`.

4. Press `Ctrl+L` on non–VS Code family terminals (or run `/hooks` anywhere — preferred on VS Code family) and check the Hooks tab to confirm it loaded.

---

## Hook Locations

Hooks are discovered from several places (all are merged):

| Scope | Path | Trusted? | Notes |
|-------|------|----------|-------|
| Global | `~/.grow/hooks/*.json` | Always | Personal hooks |
| Project | `<project>/.grow/hooks/*.json` | Requires trust | Per-repo automation |
| Config | `$GROW_HOME/config.toml` | Always | Hooks declared alongside the rest of your config |
| Plugin | Bundled inside installed plugins | Per-plugin | Shared team hooks |

Config-file hooks live in the same TOML as the rest of your Grow settings; see [Hooks in Config Files](#hooks-in-config-files) for the format.

**Trusting a project**: The first time you open a project with hooks, you must trust it before its project hooks will run -- until then they are silently skipped. Grant trust by running `/hooks-trust` (or launching with `--trust`); the decision is recorded in the unified folder-trust store (`~/.grow/trusted_folders.toml`), the same gate that governs repo-local MCP/LSP servers. Global hooks in `~/.grow/hooks/` are always trusted and need no entry. This prevents untrusted repos from running arbitrary code.

Because hooks are unified under folder-trust, a `--trust` / `/hooks-trust` grant trusts the whole folder for **MCP, LSP, and hooks** together, and cascades to subdirectories. Conversely, disabling folder-trust (`GROW_FOLDER_TRUST=0` or `[folder_trust] enabled = false`) ungates project hooks along with MCP/LSP.

---

## Hook Events

| Event | When it fires | Blocking? |
|-------|---------------|-----------|
| `session_start` | A session starts. | No |
| `user_prompt_submit` | You submit a prompt. | No |
| `pre_tool_use` | A tool is about to run. | Yes — can deny |
| `post_tool_use` | A tool completes successfully. | No |
| `post_tool_use_failure` | A tool fails. | No |
| `permission_denied` | The permission system denies a tool call. | No |
| `stop` | An agent turn ends on a genuine completion (not on a user interrupt). | Yes — can block the stop |
| `stop_failure` | A turn ends because of an API error. | No |
| `stop_cancelled` | A turn is cancelled by the user (Esc / Ctrl+C) or aborted mid-run. | No |
| `Notification` | The agent sends a notification. | No |
| `subagent_start` | A subagent starts. | No |
| `subagent_stop` | A subagent's turn ends (fires once, in the subagent, with stop decision control). | Yes — can block the stop |
| `pre_compact` | Conversation compaction is about to run. | No |
| `post_compact` | Conversation compaction completes. | No |
| `session_end` | The session ends. | No |

`pre_tool_use` can block a tool call, and `stop`/`subagent_stop` can block the Agent from stopping (see [Stop Decision Control](#stop-decision-control)); every other event is passive. Event keys are exact snake_case values; alternate spellings are rejected.

Handlers form one ordered policy chain: file handlers run first in their frozen registration order, followed by client callbacks in registration order. Grow starts only the handler whose turn has arrived. The first explicit deny, stop block, or force-stop short-circuits the chain; later handlers are recorded as skipped and are never invoked. This matters for hooks with external side effects: a later callback cannot run in the background after an earlier policy decision has already won.

---

## The Hook JSON Format

Each `.json` file can define hooks for multiple events:

```json
{
  "hooks": {
    "pre_tool_use": [
      {
        "matcher": "run_terminal_command",
        "hooks": [
          { "type": "command", "command": "bin/safety-check.sh", "timeout": 10 }
        ]
      }
    ],
    "post_tool_use": [
      {
        "hooks": [
          { "type": "command", "command": "bin/log-activity.sh" }
        ]
      }
    ]
  }
}
```

### Key Fields

- **Event name** (top-level key): any event listed in [Hook Events](#hook-events). Unrecognized event names are rejected.
- **matcher** (optional): A regular expression that selects which invocations trigger the hook. What it tests depends on the event: the tool name on tool events (`pre_tool_use`, `post_tool_use`, `post_tool_use_failure`, `permission_denied`), the notification type on `Notification`, the subagent type on `subagent_start`/`subagent_stop` (e.g. `explore`), the start source on `session_start` (`startup`, `resume`, …), the end reason on `session_end`, the compaction trigger on `pre_compact`/`post_compact` (`manual` or `auto`), and the error type on `stop_failure` (`rate_limit`, `authentication_failed`, `invalid_request`, `server_error`, `context_window_exceeded`, or `unknown`). A matcher on `stop`, `stop_cancelled`, or `user_prompt_submit` is ignored with a warning (those events always fire). An empty or omitted matcher matches everything. The matcher tests the real tool name; MCP calls routed through the internal `use_tool` dispatcher appear as the qualified `server__tool` name (e.g. `linear__save_issue`), so match on that, not the dispatcher name.
- **type**: `"command"` (run a script or shell one-liner) or `"http"` (POST the event to a URL).
- **command**: Path to executable (relative to the JSON file) or inline shell command.
- **timeout**: Seconds before killing the hook (default: 5, or 600 for `stop`/`subagent_stop` gates). All hook failures (timeouts, crashes, malformed output, missing required env vars) are fail-open: the failure is recorded for the UI scrollback but the tool call is not blocked. Only an explicit `deny` decision returned by the hook blocks a tool call.

---

## Hooks in Config Files

Hooks can also live directly in your Grow config instead of shipping separate JSON files. The
`hooks` object is read from the global config:

| File | Tier | Who sets it |
|------|------|-------------|
| `$GROW_HOME/config.toml` | Global | You |

The TOML is structurally identical to the JSON hook object, so an existing hook transliterates directly:

```toml
[[hooks.pre_tool_use]]
matcher = "run_terminal_command|search_replace"
hooks = [
  { type = "command", command = "/opt/guard/pretooluse.sh", timeout = 10 },
]
```

Each matcher group is a `[[hooks.<Event>]]` entry with an optional `matcher` and an inner `hooks` array of handlers. The handler fields (`type`, `command`, `url`, `timeout`, `env`) and event names are exactly the same as the [JSON format](#the-hook-json-format).

TOML offers two equivalent notations for the inner handlers, and both parse to the identical structure. The inline-table array shown above is recommended: it reads best for the common single-handler case. The nested array-of-tables form is also accepted:

```toml
[[hooks.pre_tool_use]]
matcher = "Bash|Write|Edit"
[[hooks.pre_tool_use.hooks]]
type = "command"
command = "/opt/guard/pretooluse.sh"
timeout = 10
```

Prefer the inline form to avoid repeating the `[[hooks.<Event>.hooks]]` header for each handler.

- **Additive across sources.** Config-file hooks and file-based hooks both run. A hook defined identically in more than one source is deduplicated, keeping the config copy.
- **Provenance labels.** Config hooks appear in `/hooks` tagged by origin (`user:`) so you can see which source contributed each one.
- **No read-time expansion.** A literal `${VAR}` in a `command` or `url` reaches the hook runner unchanged, matching JSON hook-file semantics; the runner performs the single expansion.

---

## Writing Hook Scripts

### Input

The event is sent as JSON on **stdin** (for example, a `pre_tool_use` event; the payload also always includes `toolUseId` and `toolInputTruncated`):

```json
{
  "hookEventName": "pre_tool_use",
  "sessionId": "abc-123",
  "cwd": "/Users/you/project",
  "workspaceRoot": "/Users/you/project",
  "permissionMode": "default",
  "toolName": "run_terminal_command",
  "toolInput": { "command": "npm test" },
  "timestamp": "2026-04-14T12:00:00Z"
}
```

Every event carries the same common fields: `hookEventName`, `sessionId`, `cwd`, `workspaceRoot`, `timestamp`, and `permissionMode` (`default`, `auto`, `plan`, or `bypassPermissions`), plus event-specific fields like `toolName` above.

### Output (Blocking Hooks)

For `pre_tool_use` hooks, write JSON to **stdout**:

- **Allow**: `{"decision": "allow"}`
- **Deny**: `{"decision": "deny", "reason": "Unsafe command detected"}`

### Exit Codes

| Exit Code | Meaning |
|-----------|---------|
| `0` | Success / allow (for blocking hooks) |
| `2` | Explicit deny (`pre_tool_use`) or block-stop with stderr as feedback (`stop`/`subagent_stop`) |
| Other | Fail-open — the failure is recorded but nothing is blocked. For `pre_tool_use`, a `deny` decision in stdout JSON is honored regardless of exit code. For `stop`/`subagent_stop`, valid decision JSON on stdout wins over the exit code; the exit code decides only when stdout has no usable JSON, in which case exit 2 blocks with stderr as feedback. |

### Stop Decision Control

`stop` and `subagent_stop` hooks run when the Agent is about to finish its turn and can keep it working. Write JSON to **stdout**:

- **Block the stop**: `{"decision": "block", "reason": "The test suite hasn't been run yet"}`. The reason is fed back to the model as a user message and the agent runs another round in the same turn.
- **Non-error feedback**: `{"additionalContext": "Run the linter before finishing"}`. Also keeps the Agent working, but is surfaced as hook feedback rather than a hook error.
- **Force stop**: `{"continue": false, "stopReason": "Budget exhausted"}`. Ends the turn when this is the first decisive result reached in the ordered chain.
- **Allow the stop**: exit 0 with no output (or any non-JSON output).

Exiting with code `2` also blocks the stop, with **stderr** as the feedback.

The hook input includes `stopHookActive` and `lastAssistantMessage`. `stopHookActive` is true when the agent is already continuing due to a previous stop-hook block this turn; check it, or the transcript, to avoid blocking on a condition that will never resolve. `lastAssistantMessage` carries the text of the agent's final response this turn, so hooks can act on it without parsing the transcript. After **8 continuations** (blocks or non-error feedback) in one turn the gate is overridden and the turn ends. Grow records that final Stop occurrence with every matching handler skipped by policy, but does not invoke external hooks. The counter is per turn: the next user prompt starts fresh, so a long-running goal can span turns. Hook failures fail open: the agent stops normally.

`stop` and `subagent_stop` hooks default to a 600-second timeout because gates commonly run builds or test suites, and a timed-out hook fails open. Other events keep the 5-second default. Set `timeout` explicitly when a gate needs more: `{ "type": "command", "command": "bin/verify.sh", "timeout": 1200 }`.

The gate runs only for genuine completions. Interrupted (Esc / Ctrl+C), refused, and max-turns turns skip Stop hooks entirely, and API-error turns fire `stop_failure` instead. A separate Stop also fires at session end (`reason: "channel_closed"` or `"shutdown"`); its decision output is parsed but ignored, since there is no turn left to continue. A script that counts or gates on Stop fires should check `reason == "end_turn"` so the session-end fire doesn't skew it.

`stop_failure` is observation-only (use it to log failures or send alerts; output and exit code are ignored). Its input carries `error` (one of `rate_limit`, `authentication_failed`, `invalid_request`, `server_error`, `context_window_exceeded`, or `unknown`; capacity errors fold into `rate_limit`), `errorDetails` (the raw error detail, when available), and `lastAssistantMessage` (the rendered error text shown in the conversation; for this event it is the error string, not assistant output).

`stop_cancelled` is observation-only (output and exit code are ignored). It fires after the cancelled turn's terminal is fully resolved, so a hook can never delay the cancel itself. Its input carries `reason` (one of `hook_denied`, `permission_rejected`, `permission_cancelled`, `permission_timed_out`, `mid_turn_abort`) and optionally `trigger` (the free-text cancel trigger, e.g. `ctrl_c`, `esc`). A matcher on it is ignored with a warning.

`stop` input also carries `backgroundTasks` and `sessionCrons`, so a hook can distinguish "session is done" from "session is paused waiting for background work to wake it back up". Both arrays are empty when nothing is in flight or scheduled. Each `backgroundTasks` entry describes one in-flight task: `id`, `type` (`shell`, `monitor`, or `subagent`), `status`, and (depending on the type) `command` (shell tasks only), `description` (a monitor's watched command line, or a subagent's task description), and `agentType` (subagents). Each `sessionCrons` entry describes one scheduled wakeup (`scheduler_create` or `/loop`): `id`, `schedule`, `recurring`, and `prompt`. The `schedule` value is a human-readable interval such as `every 5 minutes`; grow schedules are intervals, not cron expressions. Free-text entry fields are capped at 1000 characters with an in-string `… [+N chars]` marker.

Inside a subagent, the gate fires as `subagent_stop` (agent-frontmatter `stop` hooks are automatically remapped). A `stop` hook only gates the main agent.

`subagent_stop` fires once per subagent, at the child's own turn end. Its input carries a `phase` field (currently always `"gate"`) reserved for future protocol evolution.

A complete keep-working policy in one script:

```bash
#!/bin/bash
input=$(cat)
# Gate only genuine turn ends, not the session-end observe fire.
if [ "$(echo "$input" | jq -r '.reason')" != "end_turn" ]; then exit 0; fi
if ! bin/verify.sh >/dev/null 2>&1; then
  echo '{"decision": "block", "reason": "verify.sh failed; fix the failures before finishing"}'
fi
```

registered as `{ "type": "command", "command": "bin/stop-gate.sh", "timeout": 300 }` with `timeout` sized for the verify step. The hook fires again after each continuation, and the built-in cap ends the turn after 8; check `stopHookActive` to give up earlier on feedback the agent evidently cannot act on.

### Passive Hooks

For events like `session_start` or `post_tool_use`, stdout is ignored. Just exit 0 on success.

### Environment Variables

Grow sets several environment variables on every hook process. These are useful when writing context-aware or plugin-aware hook scripts.

#### Runner-injected variables (always available)

These variables are set by the hook runner for **every** hook:

| Variable              | Description |
|-----------------------|-------------|
| `GROW_HOOK_EVENT`     | The name of the event that triggered the hook (e.g. `pre_tool_use`, `session_start`, `post_tool_use`, `session_end`, `stop`, `notification`). |
| `GROW_HOOK_NAME`      | The configured name of this specific hook (includes the plugin prefix for plugin-provided hooks). |
| `GROW_SESSION_ID`     | The unique identifier of the current Grow session. |
| `GROW_WORKSPACE_ROOT` | Absolute path to the root of the current workspace. |

These variables are **reserved**. Any values you attempt to set for them via the `env` field in your hook JSON are stripped at load time (a warning is logged), and the runner always injects the real values at spawn time.

#### Plugin hook variables

When a hook originates from a plugin, Grow additionally injects the following variables:

| Variable             | Description |
|----------------------|-------------|
| `GROW_PLUGIN_ROOT`   | Absolute path to the plugin's installed directory. |
| `GROW_PLUGIN_DATA`   | Absolute path to the plugin's writable data directory (for storing plugin state, caches, etc.). |

These values are provided by the plugin system. The plugin adapter ensures the official `GROW_PLUGIN_ROOT` and `GROW_PLUGIN_DATA` values always win over any user-declared values in the hook's `env` map.

#### User-defined environment variables

You can supply additional environment variables for an individual hook handler using the `env` field:

```json
{
  "type": "command",
  "command": "bin/my-hook.sh",
  "env": {
    "MY_SECRET": "value",
    "LOG_LEVEL": "debug"
  }
}
```

These variables are passed through to the hook process, but they cannot override the reserved runner or plugin variables listed above.

#### Using variables in `command` and `url` fields

Both `command` and `url` support `${VAR}` and `$VAR` expansion. See the custom-hooks reference for full details on load-time vs runtime expansion, the `env` map lookup order, and how parameter-expansion modifiers (e.g. `${VAR:-default}`) are handled.

---

## HTTP Hooks

Instead of a local script, call a remote endpoint:

```json
{ "type": "http", "url": "https://hooks.example.com/grow-event", "timeout": 15 }
```

The full event envelope is POSTed as JSON.

---

## Managing Hooks in the TUI

### The Hooks Tab

Press `Ctrl+L` on non–VS Code family terminals to open the Extensions modal (Plugins tab), or run `/hooks` (any terminal; required on VS Code family where `Ctrl+L` is interject) to open it on the Hooks tab. In the **Hooks** tab:

| Key | Action |
|-----|--------|
| `r` | Reload all hooks from disk |
| `a` | Add a custom hook by path |
| `x` | Remove the selected hook source (asks for confirmation; press lowercase `y` to confirm) |
| `Space` | Enable or disable the selected hook |
| `f` | Cycle the status filter (All / Enabled / Disabled) |

Hooks are grouped by source: **Global**, **Project**, **Plugin**, and **Custom**.

Each hook shows:
- **Event** it triggers on
- **Command** or **URL** that runs
- **Timeout** duration
- **Status** -- enabled or `[disabled]`

### Slash Commands

```
/hooks-list           # Show hooks loaded in this session
/hooks-trust          # Trust this project for hook execution
/hooks-add <path>     # Add a custom hook file or directory
/hooks-remove <path>  # Remove a custom hook
/hooks-untrust        # Revoke trust for this project
```

In the TUI pager, the individual `/hooks-*` commands do not appear in the slash-command list. The `/hooks` modal covers listing, adding, removing, and enabling or disabling hooks; project trust is managed via `/hooks-trust` (or the modal's Trust action), which writes the unified folder-trust store described above.

### Per-Hook Enable/Disable

Enable or disable an individual hook at runtime by pressing `Space` in the Hooks tab. The change takes effect immediately, without restarting the session.

### Mid-Session Reload

Press `r` in the Hooks tab to reload all hooks from disk. Grow re-reads every hook source, so this picks up changes you made to hook files during the session.

---

## Hook Annotations in Scrollback

When hooks execute, their results appear as annotations in the TUI scrollback. You can see which hooks ran, whether they allowed or denied an action, and any output they produced. These annotations appear only when the plugins UI is enabled (the default).

---

## Example: Safe Shell Guard

Block dangerous shell commands:

```json
{
  "hooks": {
    "pre_tool_use": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "bin/safe-shell.sh", "timeout": 5 }
        ]
      }
    ]
  }
}
```

Where `bin/safe-shell.sh`:

```bash
#!/bin/sh
INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.toolInput.command // empty')

# Block destructive patterns
if echo "$CMD" | grep -qE '(rm -rf /|mkfs|dd if=|:(){ :|& };:)'; then
  echo '{"decision": "deny", "reason": "Blocked potentially destructive command"}' 
  exit 2
fi

echo '{"decision": "allow"}'
```

---

## Security Notes

- Global hooks (`~/.grow/hooks/`) run with your user permissions -- treat them like shell scripts.
- Project hooks require folder trust (`/hooks-trust` or `--trust`, the same gate as repo-local MCP/LSP) to prevent supply-chain attacks from malicious repos.
- HTTP hooks send session data -- only use trusted endpoints.

---

## Best Practices

1. **Keep hooks fast** -- long-running hooks block the UI. Use background processes (`&`) or async where possible.
2. **Use explicit `deny` to block** -- hooks fail-open on any error, so a hook that crashes will not block the tool. To enforce policy, your hook must run to completion and emit `{"decision":"deny","reason":"..."}` on stdout. Always handle errors inside your script so it can return an explicit decision.
3. **Use absolute paths or relative to hook file** -- scripts in `bin/` next to the JSON file are portable.
4. **Test with the modal** -- press `Ctrl+L` (non–VS Code family) or run `/hooks` to verify hooks are loaded and matching before relying on them.
5. **Version control project hooks** -- commit `.grow/hooks/` (but never secrets).

---

## Troubleshooting

- **Hook not running?** Press `Ctrl+L` on non–VS Code family (or run `/hooks` anywhere) to see if it is loaded and matched.
- **Project hooks ignored?** The folder may be untrusted. Run `/hooks-trust` (or relaunch with `--trust`).
- **Script not found?** Check the path is relative to the `.json` file and executable (`chmod +x`).
- **See errors?** Capture logs by launching with `RUST_LOG=debug GROW_LOG_FILE=/tmp/grow.log grow`, then check `/tmp/grow.log`.
