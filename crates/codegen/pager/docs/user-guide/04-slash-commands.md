# Slash Commands

Type `/` in the prompt to open the command menu. It fuzzy-matches as you type, and picking a command runs it immediately.

Commands come from two places: **shell builtins**, handled by the agent backend (shell), and **pager builtins**, handled by the TUI frontend (pager). Both show up in the same menu, and any enabled skill with `user-invocable: true` appears there too.

Every command below lists its aliases where it has them. A few commands only appear when a feature or session state enables them; those cases are called out inline. The menu is also filtered by render mode — see [`/minimal` and `/fullscreen`](#minimal-and-fullscreen).

---

## Session Management

### `/new`

Start a fresh session and clear the current conversation. Alias: `/clear`.

### `/resume`

Open the session picker to reload a previous session from disk.

### `/dashboard`

Open the [Agent Dashboard](23-dashboard.md): live roster of top-level sessions in this pager (peek, reply, dispatch, pin, rename, stop, attach). Aliases: `/agents-dashboard`, `/sessions`.

Not `/config-agents` (alias `/agents`), which manages agent *definitions* and personas. Hidden in minimal mode; disable with `GROW_AGENT_DASHBOARD=0` or `[dashboard].enabled = false`.

### `/compact [context]`

Compress conversation history to reclaim context-window space. Pass a note to tell Grow what to keep:

```
/compact
/compact keep the auth implementation details
```

Grow also auto-compacts once the context window hits 85% (tune it with `[session] auto_compact_threshold_percent`).

### `/context`

Show how the context window is being used: a category breakdown (system prompt, messages, reasoning and overhead, free space) plus informational rows for tool definitions, the skills listing, and MCP server announcements with their estimated token cost.

### `/session-info`

Show session details — auth method, model, turn count, and context usage. Aliases: `/status`, `/info`.

### `/fork`

Branch the current session into a new agent, keeping history up to this point.

### `/rewind` (alias: `/undo`)

Roll the conversation back to an earlier turn and discard everything after it. `/undo` is the same command.

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

Quit the application. Alias: `/exit`.

### `/home`

Leave the current session and return to the welcome screen. Alias: `/welcome`.

### `/delete`

Delete the current session's history and return to the welcome screen. Confirms first.

To delete a session you are not in, open `/resume` or the welcome session list and press `d` then `y`. On the dashboard, press `Ctrl+X` twice or click `[✗]`.

### `/rename`

Rename the current session. Alias: `/title`.

```
/rename new session title
```

---

## Session Selection

The selector commands below open a compact picker when invoked without arguments. Inline Slash completion and the `Ctrl+X` picker use the same catalog, ordering, availability gates, and execution path.

### `/model [name] [effort]`

Switch models. Accepts a model ID or display name (case-insensitive), and for reasoning models you can add an effort level as a second argument. Alias: `/m`.

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

### `/behavior [normal|clarify|plan|workflow|deep-research|goal]`

Choose the primary Agent's collaboration protocol. `/normal`, `/clarify`, `/plan`, `/workflow`, `/deep-research`, and `/goal` are idempotent one-step selections with a `[behavior]` prefix. Runtime-dependent Behaviors are omitted when unavailable. Leaving unfinished Plan or active Deep Research shows an interruption warning; select the same target Behavior again within the displayed window to confirm. Ordinary Enter/Esc input never confirms a Behavior transition. An unfinished Goal remains exclusive until completion or `/goal clear`.

These commands modify only the current session. Persistent defaults remain in Settings and affect future sessions only.

### `/multiline`

Toggle multiline input. When it's on, `Enter` inserts a newline and `Shift+Enter` (or `Alt+Enter`) sends the message. Mid-turn, a bare `Enter` on an empty composer still steers the top queued prompt into the active turn. Alias: `/ml`.

### `/history`

Open prompt-history search: fuzzy-search this session's prompts newest-first, then press `Enter` or `Tab` to drop a match back into the prompt.

For quick recall, press `↑` on an empty prompt instead. The panel opens with your most recent prompt already filled in; `↑`/`↓` step through entries (each lands in the input), `↓` past the newest entry closes the panel, and typing edits the recalled prompt in place.

### `/compact-mode`

Toggle compact display — less padding and tighter spacing for denser output.

### `/vim-mode`

Toggle vim-style scrollback keys (`j`/`k`, `h`/`l`, `g`/`G`, `y`/`Y`, and so on). With it off (the default), a bare letter or `Shift+letter` in the scrollback just focuses the prompt and types the character. The setting persists to `[ui] vim_mode`.

### `/minimal` and `/fullscreen`

Reopen the current session in the other render mode. `/minimal` (offered while you're in fullscreen) switches to the experimental scrollback-native mode; `/fullscreen` (offered while you're in minimal; alias `/full`) switches back to standard fullscreen mode. Both relaunch the pager on the same conversation for this session only — they don't touch `config.toml`, and the relaunch banner reminds you how to switch back. The `--minimal` / `--fullscreen` CLI flags are session-scoped the same way. To make plain `grow` open in a given mode by default, use `/settings` → **Default screen mode** or set `[ui] screen_mode`.

A handful of commands only work in one of the two modes, because the surface they drive doesn't exist in the other: `/find`, `/jump`, `/timeline`, `/theme`, `/tutorial`, `/workflows`, and `/dashboard` are fullscreen-only, while `/expand` and `/edit-prompt` are minimal-only. Those are hidden from the command menu and the palette in the mode they can't run in. If you type one out anyway, Grow says why — and points you at whichever is actually useful. When the other mode is the only way to get it, that's the mode switch: `/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.` When this mode already does the job another way, it names that instead: `/expand isn't available in fullscreen mode — press Tab to focus the scrollback, then → on the block.` Everything else works in both. Note that `--no-alt-screen` still counts as fullscreen here, so it keeps the fullscreen-only commands.

### `/plan`

Enter plan mode.

```
/plan [description]
```

### `/view-plan`

Open a preview of the current saved plan. Aliases: `/show-plan`, `/plan-view`.

---

## Memory

`/flush`, `/dream`, and `/memory` require memory to be enabled (`--experimental-memory` or `GROW_MEMORY=1`); `/memory` also needs a configured memory backend. `/remember` is always available.

### `/memory`

Browse, view, and manage saved memories. Pass `on` or `off` to enable or disable memory. Alias: `/mem`.

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

Enter Goal Behavior or manage a persistent objective. A bare `/goal` selects Goal Behavior; use `/goal resume` to resume a paused goal (a budget-limited goal must be re-budgeted first). When no goal exists, the next non-empty message becomes the objective. Outside Goal Behavior, `/goal set <objective>` switches to Goal and creates it. While a Goal is unfinished, `set` is hidden and rejected; use `/goal edit <objective>` to revise the objective and restart background planning. Tab completion after `/goal edit` pre-fills the complete current objective for editing. Later ordinary messages add constraints or evidence and never silently replace the objective or plan.

Grow works across rounds and only marks the goal complete after an independent verifier returns `Achieved`. Missing verification, timeout, infrastructure failure, insufficient evidence, exhausted attempts, or exhausted budget pauses the goal; the Agent cannot self-report completion.

```
/goal set Migrate the auth module to the new API
/goal set Migrate the auth module to the new API --budget 500000
/goal edit Migrate the auth module and preserve the legacy API
/goal budget 800000
/goal status
/goal pause
/goal resume
/goal clear
```

Arguments are `set <objective> [--budget <tokens>]`, `edit <objective> [--budget <tokens>]`, `budget <tokens>`, or one of `status`, `pause`, `resume`, `clear`. `set` is only valid outside Goal when no unfinished Goal exists. `edit` advances the objective revision, invalidates old verification evidence, cancels the matching planner/verifier lease, and returns the Goal to Planning. The `--budget` here is a **token** budget for the goal run, separate from workflow child-call budgets. `/goal budget <tokens>` adjusts the budget mid-run and also unlocks a budget-exhausted goal — run `/goal resume` to continue afterwards. `/goal pause` keeps Goal Behavior selected; `/goal clear` removes the tracker and returns to Normal. Goal is only offered when orchestration and an independent verifier are configured.

### `/deep-research [query]`

Enter Deep Research Behavior. With no query it waits for the next non-empty message; with a query it immediately starts the private read-only research run. It plans bounded, domain-adaptive research axes, gathers traceable source evidence, and checks source support, independent corroboration, and material conflicts per finding. Its evidence strategy and report structure follow the research goal instead of a fixed paper template.

```
/deep-research Compare the migration risks of PostgreSQL 17 and MySQL 9
```

The command returns right away. While it runs, ordinary messages do not start another turn or a second research run. Use `/workflows` for status and `/workflow-run pause|resume|stop` for the owned runtime. Natural completion prints the investigation and verification summary, core conclusions, limitations, and the absolute path to a complete Markdown report. That artifact is coverage-driven rather than length- or section-driven, may use cited tables, Mermaid diagrams, or verified external images when useful, and includes the sources actually used. Cancellation, budget exhaustion, interruption, and runtime failure still produce a terminal report. Natural completion returns the session to Normal. Deep Research is private and cannot be launched from `/workflow-run`.

### `/workflow [prompt]`

Enter Static Workflow Behavior, optionally sending the prompt after the Behavior switch succeeds. Static Workflow Behavior lets the primary Agent scout, author one bounded scripted workflow for the current phase, launch parallel children, verify the result, and choose the next phase without a whole-plan approval boundary.

Model-launched workflows may set `agent_budget` and `max_concurrency` independently. `agent_budget` is an absolute cumulative cap on logical child-agent calls: every `agent()` call and every item in a `parallel()` panel spends one slot, while schema-correction retries don't. The default is 128 and explicit values run 1–1,024. `max_concurrency` bounds simultaneous children, defaults to 3, and accepts 1–16. Queued children remain cancellable. Named slash launches use both defaults.

### `/workflow-run`

Launch a saved workflow, or manage a running one by the session-unique display name shown in `/workflows`. Launch the same workflow twice and the display names are numbered (`review-changes`, `review-changes-2`); you never need the internal run IDs.

```
/workflow-run review-changes {"target":"origin/main...HEAD"}
/workflow-run pause review-changes
/workflow-run resume review-changes
/workflow-run stop review-changes-2
/workflow-run save review-changes
```

Project workflows live in `.grow/workflows/*.rhai`; user workflows live in `~/.grow/workflows/*.rhai`. A same-process pause/resume continues the original immutable script, args, and `agent_budget` cap from committed host-call results — to iterate, edit the returned script copy and launch it as a new run.

A budget-limited run is different: it only resumes through a model/tool resume request that supplies an `agent_budget` above the admitted agent count. A bare `/workflow-run resume <name>` can't raise the cap, so it rejects budget-limited runs. Runs interrupted by a process restart aren't resumed at all, because external effects have no stable cross-process identity. And resume is not exactly-once: an external effect whose result wasn't committed before a same-process pause can run again.

### `/workflows`

Open the live workflows **run** dashboard — active and retained runs, not a catalog of saved definitions. Each row shows the run's display name, phase, agent roster, progress, and result. Inside a run's detail view, `p` pauses, `r` resumes an ordinary pause, and `x` stops. Budget-limited runs can't bare-resume: `r` returns the shell's rejection (raise the cap with a model/tool resume that passes a higher `agent_budget`), while `x` still stops. `s` saves the run's script, but it's hidden for known built-ins and numbered duplicate handles — for those, choose a new unique `meta.name` and save the edited script explicitly.

---

## Other

### `/shortcuts`

Open the searchable keyboard-shortcuts modal. This is the only entry point.

### `/theme`

Switch the color theme. Alias: `/t`.

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

Check the current session for terminal, clipboard, color, input, notification, and sandbox issues. Doctor shows what it found and how to resolve each issue. Run `/doctor fix` to list available automatic fixes; other findings include manual steps. `/terminal-setup`, `/terminal-check`, and `/terminal-info` remain aliases.

### `/release-notes`

View release notes for the current version. Alias: `/changelog`.

### `/docs`

Browse the built-in How-to Guides, open the online Build docs, or jump straight to a guide by title. Aliases: `/howto`, `/guides`.

```
/docs
/docs web
/docs Getting Started
```

- Bare `/docs` (or `/docs how-to`) opens the How-to Guides picker.
- `/docs web` opens https://docs.example.com/build/overview in your browser.
- `/docs <title>` opens a specific guide by case-insensitive title match.

### `/tutorial`

Open the onboarding tutorial: a short list of topics (your first prompt, attaching context, navigation, slash commands, worktrees, plan mode, customization, switching from another agent tool) — each a ~30-second read, with `→` flowing straight to the next topic. Nothing auto-shows — this command (or the command palette) is the way in.

```
/tutorial
```

Aliases: `/tour`, `/onboarding`

### `/import-claude`

Open the Claude import modal to bring over `~/.claude` settings: permissions, environment variables, MCP servers, hooks, and paths.

---

## Agents and Personas

### `/config-agents`

Open the configuration surface for Agent/persona definitions.

### `/agents`

Open the Agent Dashboard. This is the only formal dashboard Slash command; removed aliases such as `/dashboard` and `/sessions` are not retained.

Not the live multi-session [Agent Dashboard](23-dashboard.md) (`/dashboard` / `Ctrl+\`).

### `/personas`

Create, edit, and delete personas. A subagent can apply a persona to shape how it behaves.

---

## Account and Usage

### `/usage`

View local token and context usage for the current session. Alias: `/cost`.

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

Open the settings modal to view and change configuration interactively. Aliases: `/config`, `/preferences`, `/prefs`.

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
