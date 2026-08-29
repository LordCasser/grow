# Slash Commands

Type `/` in the prompt to open the command menu. It fuzzy-matches as you type, and picking a command runs it immediately.

Commands come from pager and Shell builtins, enabled Skills, saved Workflows, and ACP extensions. They share one menu. Each row carries a purpose label — **SESSION**, **CONTROL**, **RUN**, **VIEW**, **SETTINGS**, or **EXTENSION** — while Skill and Workflow remain source metadata rather than a second command hierarchy.

The menu lists canonical command names. Grow registers no production aliases; in particular, history rollback is `/rewind`, not `/undo`. A few commands only appear when a feature or session state enables them; those cases are called out inline. The menu is also filtered by render mode — see [`/minimal` and `/fullscreen`](#minimal-and-fullscreen).

---

## Session Management

### `/new`

Start a fresh session and clear the current conversation.

### `/resume`

Open the session picker to reload a previous session from disk.

### `/dashboard`

Open the [Agent Dashboard](23-dashboard.md): live roster of top-level sessions in this pager (peek, reply, dispatch, pin, rename, stop, attach).

Not `/config-agents`, which manages Agent definitions. Hidden in minimal mode; disable with `GROW_AGENT_DASHBOARD=0` or `[dashboard].enabled = false`.

### `/compact [context]`

Compress conversation history to reclaim context-window space. Pass a note to tell Grow what to keep:

```
/compact
/compact keep the auth implementation details
```

Grow also auto-compacts once the context window hits 75% (tune it with `[session] auto_compact_threshold_percent`).

### `/context`

Show how the context window is being used: a category breakdown (system prompt, messages, reasoning and overhead, free space) plus informational rows for tool definitions, the skills listing, and MCP server announcements with their estimated token cost.

In fullscreen and inline modes this opens the tabbed usage modal (Usage · Context · Session Info); `Esc` closes it and nothing is written into the transcript. Minimal mode keeps the inline scrollback output.

### `/session-info`

Show session details — auth method, model, turn count, and context usage.

In fullscreen and inline modes this opens the tabbed usage modal on the Session Info tab, where each row (Session ID, working directory, model, …) can be clicked to copy. Minimal mode keeps the inline scrollback output.

### `/trajectory`

Open an independent local web debugger for the exact active session. The page
tails its durable parent and Sideband Timelines, provides causal lanes and
filters, and keeps running independently of the TUI. This command never falls
back to the most recently used session, so the page identity always matches the
session from which it was opened.

### `/fork`

Branch the current session into a new agent, keeping history up to this point.

### `/rewind`

Roll the conversation back to an earlier turn and discard everything after it.

### `/edit-prompt`

In minimal mode, open an external editor for an empty composer. Grow resolves `$VISUAL`, then `$EDITOR`, then `vi`; command values may include quoted arguments. Saving replaces the draft without sending it, and saving an empty file clears it. The command is hidden outside minimal mode.

```
/edit-prompt
```

To edit an **existing** draft when a terminal or multiplexer reserves `Ctrl+G`, open the command palette and select **Edit Prompt in External Editor**. That direct route preserves the existing text and refuses pasted, file-reference, or image chips without flattening them. Typing `/edit-prompt` into the composer necessarily replaces that input, so it starts from an empty draft.

### `/copy`

Copy the most recent response to the clipboard. Pass a number to copy the Nth-latest response instead, or a file path to write the text to a file rather than the clipboard (handy over SSH, where the local clipboard is often unreachable).

```
/copy
/copy 2
/copy out.txt
/copy 2 ~/exports/last-reply.md
```

Every copy is also written to a backup file — `~/.grow/last-copy.txt` by default, or `GROW_COPY_FILE` if set. Confirmed copies toast briefly (e.g. `Copied!`). Unverified OSC 52 deliveries and clipboard-unreachable fallbacks name the backup path so you can recover the text.

### `/export`

Export the conversation to a file or the clipboard.

### `/quit`

Quit the application.

### `/home`

Leave the current session and return to the welcome screen.

### `/delete`

Delete the current session's history and return to the welcome screen. Confirms first.

To delete a session you are not in, open `/resume` or the welcome session list and press `d` then `y`. On the dashboard, press `Ctrl+X` twice or click `[✗]`.

### `/rename`

Rename the current session.

```
/rename new session title
```

---

## Session Selection

The selector commands below open a compact picker when invoked without arguments. Inline Slash completion and the `Ctrl+X` picker use the same catalog, ordering, availability gates, and execution path.

### `/model [name] [effort]`

Switch models. Accepts a model ID or display name (case-insensitive), and for reasoning models you can add an effort level as a second argument.

```
/model deepseek/deepseek-chat
/model DeepSeek Chat
/model deepseek/deepseek-chat max
```

### `/agent [agent]`

Switch the current session's Agent without changing Behavior, model, or permissions. Agent ids come from discovery: built-ins and top-level definitions use a bare name; nested files under `~/.grow/agents/` or `.grow/agents/` use a path-style id (for example `software-engineering/software-architect`). The picker marks each row with `(system)` or `(user)` (and project/bundled when applicable). Use `/behavior` or `Ctrl+X` then `b` to change Behavior separately.

### `/effort [level]`

Set reasoning effort on the **current** model without reselecting it. The accepted values come from that model's configured `reasoning_efforts`; Grow does not assume every model supports the same levels.

```
/effort high
```

### `/permission [ask|auto|always-approve]`

Choose the current session's Permission policy. `/ask`, `/auto`, and `/always-approve` are idempotent one-step selections, identified in completion by a `[permission]` prefix. `/auto` is hidden when classifier-based permission is unavailable.

### `/behavior [normal|clarify|plan|workflow|goal]`

Choose the primary Agent's collaboration protocol. `/normal`, `/clarify`, `/plan`, `/workflow`, and `/goal` are idempotent one-step selections with a `[behavior]` prefix. Runtime-dependent Behaviors are omitted when unavailable. Leaving unfinished Plan or Workflow with an Active Run shows an interruption warning; select the same target Behavior again within the displayed window to confirm. Paused and budget-limited Runs do not require confirmation. Ordinary Enter/Esc input never confirms a Behavior transition. Active Goal must first be paused, completed, or cleared; a stopped Goal may coexist with another Behavior until restarted.

These commands modify only the current session. Persistent defaults remain in Settings and affect future sessions only.

Model and effort form one Sampling target; Agent and Behavior are separate control domains. If a domain is changed repeatedly before it applies, only the newest target remains pending. Sampling and Agent changes submitted during a running turn leave the current stream and tool batch untouched, then apply after `StepEnded` and before the next model step. Behavior keeps its own ownership, confirmation, and turn-admission rules. Fullscreen/inline show pending state in the footer; Minimal uses its live status line. Only the final applied or rejected user action becomes a transcript Notice. Loading or resuming a session silently hydrates the current values and does not print a new switch result.

### `/multiline`

Toggle multiline input. When it's on, `Enter` inserts a newline and `Shift+Enter` (or `Alt+Enter`) sends the message. Mid-turn, a bare `Enter` on an empty composer still steers the top queued prompt into the active turn.

### `/history`

Open prompt-history search: fuzzy-search this session's prompts newest-first, then press `Enter` or `Tab` to drop a match back into the prompt.

For quick recall, press `↑` on an empty prompt instead. The panel opens with your most recent prompt already filled in; `↑`/`↓` step through entries (each lands in the input), `↓` past the newest entry closes the panel, and typing edits the recalled prompt in place.

### `/compact-mode`

Toggle compact display — less padding and tighter spacing for denser output.

### `/vim-mode`

Toggle vim-style scrollback keys (`j`/`k`, `h`/`l`, `g`/`G`, `y`/`Y`, and so on). With it off (the default), a bare letter or `Shift+letter` in the scrollback just focuses the prompt and types the character. The setting persists to `[ui] vim_mode`.

### `/minimal` and `/fullscreen`

Reopen the current session in the other render mode. `/minimal` (offered while you're in fullscreen) switches to the experimental scrollback-native mode; `/fullscreen` (offered while you're in minimal) switches back to standard fullscreen mode. Both relaunch the pager on the same conversation for this session only — they don't touch `config.toml`, and the relaunch banner reminds you how to switch back. The `--minimal` / `--fullscreen` CLI flags are session-scoped the same way. To make plain `grow` open in a given mode by default, use `/settings` → **Default screen mode** or set `[ui] screen_mode`.

A handful of commands only work in one of the two modes, because the surface they drive doesn't exist in the other: `/find`, `/jump`, `/timeline`, `/theme`, `/tutorial`, `/workflows`, and `/dashboard` are fullscreen-only, while `/expand` and `/edit-prompt` are minimal-only. Those are hidden from the command menu and the palette in the mode they can't run in. If you type one out anyway, Grow says why — and points you at whichever is actually useful. When the other mode is the only way to get it, that's the mode switch: `/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.` When this mode already does the job another way, it names that instead: `/expand isn't available in fullscreen mode — press Tab to focus the scrollback, then → on the block.` Everything else works in both. Note that `--no-alt-screen` still counts as fullscreen here, so it keeps the fullscreen-only commands.

Fullscreen and inline retain a mutable status/footer layer. Minimal instead writes final blocks into the terminal's native Scrollback, so pending and applying controls stay in the single live status line and are never committed as disposable history. Applied, rejected, cancelled, and Subagent-finished terminals are committed once; relaunching between render modes restores pending state from the Shell rather than inventing another result row.

### `/plan`

Enter plan mode.

```
/plan [description]
```

### `/view-plan`

Open a preview of the current saved plan.

---

## Memory

`/flush`, `/dream`, and `/memory` require memory to be enabled (`--experimental-memory` or `GROW_MEMORY=1`); `/memory` also needs a configured memory backend. `/remember` is always available.

### `/memory`

Browse, view, and manage saved memories. Pass `on` or `off` to enable or disable memory.

```
/memory
/memory off
```

### `/flush`

Save the current session's knowledge to memory right now, triggering an LLM summary of the most important content. Reach for it before compaction, or any time you want to lock in context.

### `/dream`

Run memory consolidation — merge session logs into organized topics.

### `/remember`

Save a note to memory immediately, without waiting for an automatic summary.

```
/remember the staging deploy uses the eu-west cluster
```

---

## Hooks and Plugins

`/hooks`, `/plugins`, `/marketplace`, and `/skills` all open the same extensions modal, each on its own tab.

### `/hooks`

Open the extensions modal on the Hooks tab, where you can view loaded hooks, add or remove custom ones, and toggle them individually. The modal does not grant project trust — see [10-hooks.md](10-hooks.md) for the trust model.

The shell also advertises individual `/hooks-list`, `/hooks-trust`, `/hooks-add`, `/hooks-remove`, and `/hooks-untrust` commands; in the pager these are folded into the `/hooks` modal.

### `/plugins`

Open the extensions modal on the Plugins tab to view installed plugins, install new ones from the marketplace, and manage trust.

The shell additionally supports subcommands (`/plugins list`, `/plugins install <source>`, `/plugins uninstall <name>`, `/plugins update`, `/plugins reload`). In the pager, the modal does the same work visually.

### `/marketplace`

Open the extensions modal on the Marketplace tab to browse and install plugins.

### `/skills`

Open the extensions modal on the Skills tab to view installed skills.

---

## Scheduling

### `/loop [interval] <prompt>`

Run a prompt on a recurring interval. Give the interval as `30m`, `1 hour`, or `every 2 days`; leave it out and Grow will ask.

```
/loop 30m check deploy status
/loop check deploy status every hour
```

Intervals are `Ns` (seconds, minimum 60), `Nm` (minutes), `Nh` (hours), or `Nd` (days); anything under 60 seconds is raised to the minimum. Recurring tasks expire after 7 days, and you can cancel one with `scheduler_delete` using the job ID reported when the loop is created.

---

## Workflows and Goals

### `/goal`

Enter Goal Behavior or manage one long-lived objective. A bare `/goal` selects Goal Behavior only when no stopped Goal blocks activation; use `/goal restart` to reactivate a paused or blocked Goal. A budget-limited Goal must first receive a new budget or `/goal budget unlimited`, then be restarted. When no Goal exists, the next non-empty user message becomes the objective. `/goal set <objective>` atomically creates and activates it. While a Goal is unfinished, `set` is hidden and rejected; use `/goal edit <objective>` to revise the same Goal. Editing Paused/Blocked state does not restart it; editing BudgetLimited/Complete establishes an active revised definition. Tab completion after `/goal edit` pre-fills the full objective.

An active Goal requests another turn whenever the session becomes idle. Every continuation first audits the entire objective against concrete evidence. If work remains, the Agent uses ordinary short-lived `todo_write` steps and `task` subagents for the next verifiable slice; those tasks are execution context, not a second persistent plan or Goal state. Later user messages take priority and add constraints or evidence without silently replacing the objective.

```
/goal set Migrate the auth module to the new API
/goal set Migrate the auth module to the new API --budget 500000
/goal edit Migrate the auth module to the new API and remove the legacy API
/goal budget 800000
/goal budget unlimited
/goal status
/goal pause
/goal restart
/goal clear
```

Arguments are `set <objective> [--budget <tokens>]`, `edit <objective> [--budget <tokens>]`, `budget <tokens|unlimited>`, or one of `status`, `pause`, `restart`, `clear`. `set` is valid only when no unfinished Goal exists. `edit` preserves accumulated usage and updates the objective; Paused/Blocked remain stopped so edit and restart are independent controls. The budget is for the whole Goal, separate from Workflow child-call budgets. Changing an exhausted budget moves the Goal to Paused; run `/goal restart` afterwards. `/goal pause` stops continuation and returns the active Behavior to Normal. `/goal clear` deletes only Goal state and preserves another Behavior already selected while the Goal was stopped. Goal is offered only when `create_goal`, `get_goal`, `update_goal`, and `todo_write` are available; `task` remains optional bounded delegation.

### `/workflow [prompt]`

Enter Workflow Behavior, optionally sending the prompt after the Behavior switch succeeds. This is the only Behavior in which Grow can discover, create, modify, validate, publish, run, or manage public Workflow Definitions. Outside it, only `/workflow [prompt]` and `/behavior workflow` are offered as public Workflow entry points.

The session Workspace can keep several temporary drafts and Runs, but it has one explicit Definition focus. “Current workflow” means that focus. A saved Definition is first derived into a same-name session draft before Grow edits it; Grow never edits the Project/User file directly. Editing affects only the next Run. Session drafts persist their inline/file/Definition source and content hashes across session resume, then disappear with the session. External-editor changes to saved files remain allowed and are rediscovered on the next scan.

### `/workflow-run`

A bare `/workflow-run` opens the Workflow selector; it never starts the latest Run implicitly. `/workflow-run <definition-name> [args]` launches that saved Project/User Definition directly, as does its dynamic `/<definition-name> [args]` command. A same-name session editing draft remains visible in the Workspace but does not hide or replace the saved Definition's command. Explicit subcommands manage a Run by its session-unique handle. Launching the same Definition twice produces `review-changes`, `review-changes-2`, and so on.

```
/workflow-run review-changes compare main
/workflow-run pause review-changes
/workflow-run resume review-changes
/workflow-run stop review-changes-2
```

Every Run snapshots its Definition id, scope, content hash, script, args, and limits. A same-process pause/resume continues that immutable snapshot. Modifying or publishing a Definition never changes a running or resumable Run.

A budget-limited run is different: it only resumes through a model/tool resume request that supplies an `agent_budget` above the admitted agent count. A bare `/workflow-run resume <name>` can't raise the cap, so it rejects budget-limited runs. Runs interrupted by a process restart aren't resumed at all, because external effects have no stable cross-process identity. And resume is not exactly-once: an external effect whose result wasn't committed before a same-process pause can run again.

### `/workflows`

Open the Workflow Workspace. Definitions show focus, scope, temporary/saved, dirty, validated, and conflicted state; Runs show their Definition provenance and hash, handle, status, phase, and Agent progress. Definition actions are Focus, Inspect/Edit, Validate, Run, Publish, and Discard. Run actions are Pause, Resume, Stop, and details. Publishing always asks for Project (`.grow/workflows/<name>.rhai`) or User (`~/.grow/workflows/<name>.rhai`) scope and refuses an external-edit conflict.

---

## Other

### `/shortcuts`

Open the searchable keyboard-shortcuts modal. This is the only entry point.

### `/theme`

Switch the color theme.

### `/feedback`

Open the [Grow GitHub issue creation page](https://github.com/LordCasser/grow/issues/new) in your browser.

### `/btw`

Send an aside to the agent without interrupting the current task. In minimal mode (`--minimal`), the answer shows up in a dismissible panel above the prompt: `Esc` dismisses it, a finished answer is saved into native scrollback, and a late reply to an already-dismissed panel is dropped. The side question and its answer aren't part of the main turn.

```
/btw also check the error handling
```

### `/mcps`

Open the MCP servers management modal.

### `/doctor`

Check the current session for terminal, clipboard, color, input, notification, and sandbox issues. Doctor shows what it found and how to resolve each issue. Run `/doctor fix` to list available automatic fixes; other findings include manual steps.

### `/release-notes`

View release notes for the current version.

### `/docs`

Browse the built-in How-to Guides, open the online Build docs, or jump straight to a guide by title.

```
/docs
/docs web
/docs Getting Started
```

- Bare `/docs` opens the How-to Guides picker.
- `/docs web` opens https://docs.example.com/build/overview in your browser.
- `/docs <title>` opens a specific guide by case-insensitive title match.

### `/tutorial`

Open the onboarding tutorial: a short list of topics (your first prompt, attaching context, navigation, slash commands, worktrees, plan mode, customization, switching from another agent tool) — each a ~30-second read, with `→` flowing straight to the next topic. Nothing auto-shows — this command (or the command palette) is the way in.

```
/tutorial
```

## Agents

### `/config-agents`

Open the configuration surface for Agent definitions.

### `/agents`

Open the Agent Dashboard.

Not the live multi-session [Agent Dashboard](23-dashboard.md) (`/agents` / `Ctrl+\`).

---

## Account and Usage

### `/usage`

View local token and context usage for the current session.

In fullscreen and inline modes this opens the tabbed usage modal on the Usage tab; `Esc` closes it and nothing is written into the transcript. Minimal mode keeps the inline scrollback output.

```
/usage
```

### `/privacy`

Show or toggle privacy and data-retention status.

```
/privacy
/privacy opt-in
/privacy opt-out
```

Local diagnostic logs are independent of `/privacy`; Grow never uploads them.

---

## Configuration and UI

### `/settings`

Open the settings modal to view and change configuration interactively.

### `/timestamps`

Toggle message timestamps on or off.

---

## Skills as Slash Commands

Any enabled skill with `user-invocable: true` in its SKILL.md frontmatter shows up as a slash command. (Turn a skill off via `/skills` and it stops being advertised.) So a skill at `~/.grow/skills/commit/SKILL.md` runs as:

```
/commit fix typo in README
```

Skills from plugins work the same way. When two skills share a name across scopes, qualify it:

```
/local:commit      # Project-scoped skill
/user:commit       # User-scoped skill
```

Built-in commands always win over a skill with the same name. Name a skill "compact" and `/compact` still runs the built-in — but `/local:compact` invokes the skill.

---

## Autocomplete

The menu supports fuzzy search: start typing after `/` to filter. Each entry shows the command name, its description, an argument hint when it takes arguments, and its source (builtin, skill scope, or plugin name). Press `Tab` or `Enter` to accept the highlighted command.
