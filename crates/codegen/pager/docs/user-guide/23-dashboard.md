# Agent Dashboard

The Agent Dashboard is the overview for sessions open in this pager process.
It provides stable navigation, a read-only preview, session controls, search,
and a separate input for creating a new session. Open it with `grow dashboard`,
`/agents`, or `Ctrl+\`.

The dashboard is separate from the Agent-definition view (`/config-agents`),
the saved-session picker (`/resume` / `Ctrl+S`), and the Workflow workspace
(`/workflows`).

## Overview and navigation

The default `open` grouping keeps sessions in stable identity order. Press
`Ctrl+G` to cycle through `open` → `state` → `directory`. State and directory
views group rows under section headings; expand or collapse a heading with
`←` / `→` or click it.

Use `↑` / `↓` or `j` / `k` to move through the overview. Selecting a session
updates its read-only preview; it does not create a reply target or answer a
pending question. Press `Enter` or double-click a row to open the ordinary
session details view. That view keeps the usual editing, scrolling, and Esc/Left
behavior. Press `Ctrl+\` to return to the overview.

`Ctrl+R` renames the selected session. `Ctrl+T` pins or unpins it, and
`Shift+↑` / `Shift+↓` reorders pinned sessions.

## New session input and search

The new-session input and search query are separate drafts. Typing a prompt in
the new-session input creates a new session; it never sends a reply to the
selected row. Search mode filters the overview without replacing the prompt.
Press `Ctrl+/` to enter or leave search mode.

Search prefixes are available only in search mode:

- `a:<name>` searches Agent labels.
- `s:<state>` searches states such as `working`, `idle`, `completed`,
  `failed`, `needs-input`, and `blocked`.
- `#<text>` searches literal label text beginning with `#`.
- Any other query searches the label and working directory.

Press `Enter` to keep the filter and return to the new-session input. Press
`Esc` or `Ctrl+/` to clear the filter and leave search mode.

The dashboard stores preferences in `~/.grow/config.toml`:

```toml
[dashboard]
enabled = true
grouping = "open" # or "state" / "directory"
pinned = ["top:<session_id>"]
reorder = ["top:<session_id>"]
```

Pin and reorder entries use session IDs, so they remain attached to the same
session across pager restarts. Set `GROW_AGENT_DASHBOARD=0` to disable the
dashboard for one pager invocation.

## Session controls

`Ctrl+X` opens the shared session-command prefix. `M`, `A`, `E`, `P`, and `B`
select the model, Agent, reasoning effort, Permission, or Behavior. `C` closes
the session after the runtime confirms the close; saved history is retained.
`D` requests permanent deletion and opens a separate confirmation: press `Y`
to delete, or `N` / `Esc` to cancel. Repeating `Ctrl+X` only stops session work;
an idle session remains open.

These commands target the selected local session on the overview and the
current session in its details view. The `Ctrl+X` prefix uses the same command
entry in both views.

`/quit` quits the Grow application. It does not close the current session or
delete its history. Use `Ctrl+X C` to close a session while keeping its history.
