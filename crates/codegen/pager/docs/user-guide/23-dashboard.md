# Agent Dashboard

The Agent Dashboard lists every top-level session in this pager process —
local sessions and forks — grouped by state. From one screen you can peek,
reply, attach, pin, rename, stop, or dispatch a new agent. Subagents are not
listed; they run under their parent, which already shows when work is in
flight.

Not the Agent-definition modal (`/config-agents`), the session picker
(`/resume` / `Ctrl+S` — past conversations on
disk), or the workflows run UI (`/workflows`).

---

## Opening the dashboard

Three entry points, all opening the same view:

- **`grow dashboard`** — launches the TUI directly into the dashboard.
- **`/agents`** — the only Slash command for opening it from an active session.
- **Ctrl+\\** — same as the slash command, two keystrokes. Configurable
  in `~/.grow/config.toml` under `[keybindings]` like every other shortcut.

---

## What you see

```
 Grow · Dashboard — 4 agents · 2 awaiting
▌● reviewer · audit token flow    Awaiting your input            2m
 ● implementer · fix login bug    Running: cargo test           12m
 ⋅ refactor · feat/login          Responding…                   24m
 ○ housekeeping                   idle                           1h
 ● implementer · add login tests  8 tools · 1.2k tok            14m
╭─────────────────────────────────────────────────────────────────╮
│ ❯ Dispatch a new agent                                          │
╰─ dispatch ──────────────────────────────────────────────────────╯
 ↑/↓ select (peek) · Enter open · Ctrl+R rename · Ctrl+T pin · Ctrl+X stop · ? help · Esc new
```

Each row is a top-level agent (subagents aren't shown — they run under
their parent). Rows are sorted by state (Needs input → Working → Idle →
Inactive → Completed → Failed) so same-state rows sit adjacent, or by
working directory (toggle with `Ctrl+G`). **Inactive** holds roster-only
sessions — idle/dormant sessions owned by other pager processes that
haven't been loaded in this one — so **Idle** stays focused on the
sessions you're actively cycling between. Because it's background noise,
**Inactive starts collapsed** (expand it with `→` / click — see below).

To keep the **Idle** group scannable, only the most recent idle agents
stay visible — the 8 freshest, plus any active within the last hour. The
rest fold into a **"N more"** row (marked with a `+` / `-` toggle) at the
bottom of the group; select it and press `Enter` / `→` (or click it) to
reveal them all, and `←` to re-fold. The Idle header always shows the true total. Folding is
suspended while a filter or search is active (so every match shows).

The state icon matches Grow's sibling views (
`tasks_pane`):

- `⋅`/`:`/`⸬`/`⁙` — animated spinner for **Working** rows.
- `●` — filled circle for **Needs input**, **Completed**, **Failed**,
  **Blocked** (color: yellow / green / red / amber)
- `○` — hollow circle for **Idle** and **Inactive**

A row stays **Working** while it has live background work even if its turn
has finished — a background task, a `monitor`, or an active scheduled
`/loop`. The activity line says what is still running (for example
`1 monitor · 2 loops still running`).

There are no inline group headers; sort order keeps same-state rows adjacent,
and the per-row dot + color shows the group.

The dispatch input uses the same prompt chrome as the agent view. Press
`Ctrl+/` to flip it into **search mode**: the `❯` prefix becomes a yellow
`Search:` and typing live-filters the list instead of dispatching.

---

## Keybindings

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | Navigate rows and section titles (selecting a row opens peek) |
| `→` / `←` (on a section title) | Expand / collapse the section (`l` / `h` in vim mode) |
| `Enter` (on a section title) | Toggle the section collapsed / expanded |
| `Enter` (empty reply) | Open the selected agent full-screen (details view) |
| `Ctrl+S` | Send the peek reply and open the agent (or dispatch and attach a new session) |
| `Shift+Enter` / `Alt+Enter` | Newline in the reply / dispatch input |
| `1`–`9` | Answer a pending permission / ask question when peek shows options |
| `Enter` (typed reply) | Send / queue the reply to the selected agent |
| `/` | Literal `/` into the prompt |
| `Ctrl+/` | Toggle search mode (live-filter rows) |
| `Ctrl+R` | Rename selected row |
| `Ctrl+T` | Pin / unpin |
| `Ctrl+G` | Toggle grouping (state ↔ directory) |
| `Ctrl+X` | Cancel a running turn, or press twice within 2s to permanently delete |
| Hover + click `[✗]` | Permanently delete an idle/done row (click again to confirm) |
| `Shift+↑` / `Shift+↓` | Reorder pinned rows |
| `Esc` | Step back one level: cancel search → close peek (clear reply draft, then unselect) → clear filter → **unfocus the dispatch input** (so `↑`/`↓`, `j`/`k` navigate the list) → unselect row (→ `[+ New Agent]`) → exit dashboard. Esc never clears your typed dispatch draft — use `Ctrl+U` / `Ctrl+C` for that |
| `Ctrl+\` | Return to the dashboard from the details view, or exit dashboard |

When grouping by state, each group has a **section title** (e.g. `Working`,
`Idle`) with a `▸`/`▾` disclosure marker. Section titles are part of the
up/down navigation: select one and press `→` to expand it (showing its rows)
or `←` to collapse it — `l` / `h` do the same when vim mode is on.
**Clicking** a section title toggles it, and **hovering**
brightens its text. Collapse state is remembered while the dashboard stays open.
The **Inactive** section starts collapsed by default each time the pager
starts; expanding it sticks until you quit.

Opening a row shows the agent's conversation in the **details view**:
a single top header row (the agent name on the left, `{i}/{n} [‹][›]
[Dashboard]` cycle/close affordances on the right) sits above the conversation,
which renders **full-width** — no bordered modal frame — so the prompt
position and overall padding match the dashboard list view. All key
presses route to the attached agent; `Esc` / `Ctrl+\\` (or the `[Dashboard]`
affordance) return to the dashboard, the `[‹]` / `[›]` chips cycle to
the previous / next agent, and the agent's shortcuts bar shows a
`Ctrl+\\: back to dashboard` hint. Quick gotcha — `Esc` only returns to
the dashboard; typing `/quit` inside the agent actually closes the
underlying session (returning to the dashboard with a "Session closed"
toast).

`Ctrl+X` in the details view is state-dependent. While a **turn is
running** it cancels the turn — the same behaviour as `Ctrl+C`,
including the keep-subagents prompt — and never touches the session
itself, so mashing it to stop a turn can't close anything. In any
other state — **idle**, a slash command in flight (commands can't
be cancelled yet), or a cancel still pending — `Ctrl+X` arms a
confirmation: the shortcuts bar flips to "press Ctrl+x again to
close this session", and a second press within 2 seconds closes the
session and returns you to the dashboard. Pressing any other key
cancels the confirmation, and a turn that starts inside the window
downgrades the confirmed press to a cancel instead of closing. The
shortcuts cheatsheet remains available through `/shortcuts` inside the details
view.

All shortcuts are registered under `When::DashboardFocused` and can be
rebound via `~/.grow/config.toml`.

---

## Completing or closing a session

There is **no** “mark completed” command. Row state is derived from the agent:

- **Completed** / **Failed** when work ends on its own (turn finished and no
  background task / monitor / `/loop` still running).
- **`Ctrl+X` once** while a turn is running cancels the turn.
- **`Ctrl+X` twice** (within 2s) **permanently deletes** the session
  (same as `/delete`). Hover an idle/done row to swap age for `[✗]` and
  click twice to confirm.
- In the details view, `/quit` also closes the session (Esc only returns).
  `/delete` inside an attached agent wipes that session and returns home.

There is no manual complete flag. Use `/quit` to leave a session without
deleting history.

---

## Dispatch input

The bottom textarea **always spawns a new session**. A selected row is the
navigation cursor, not a reply target — open an agent to talk to it.

- Free text → new top-level session seeded with the prompt. Text is never
  treated as a filter (even if it starts with `/`, `s:`, `a:`, or `#`);
  filtering is `Ctrl+/` search mode. A leading `/` runs a pager-global slash
  command.
- Empty input → open the selected row, or create a new agent when
  `[+ New Agent]` is focused.

`Ctrl+S` after typing dispatches **and** attaches; plain `Enter` stays on the
dashboard so you can dispatch several sessions. `Shift+Enter` / `Alt+Enter`
insert a newline; the box grows with the draft (up to a cap, then scrolls).

Empty or whitespace-only prompts are ignored. Prompts above 64 KiB are
rejected with a toast.

### Focus: input bar ↔ overview list (`Tab`)

Two focus areas: the **dispatch input** and the **overview list**. `Tab`
toggles between them; the inactive input dims its border and hides its caret.

On open, focus defaults to the **overview list** when at least one agent
exists (so `↑`/`↓` / vim `j`/`k` navigate immediately). With **no** agents,
focus stays on the **dispatch input**. Either way, the cursor starts on
`[+ New Agent]` (no agent row pre-selected).

- **Input focused**: type a new-session prompt. Empty prompt: `↑`/`↓`
  navigate rows; non-empty: move the caret. `Esc` unfocuses to the list
  (draft kept).
- **Overview focused**: `↑`/`↓` (and vim `j`/`k`) move between rows. `Enter`
  opens the highlighted agent (on `[+ New Agent]`, sends a typed draft or
  creates a new session). `Esc` stays on the list and steps back — clear
  filter, then unselect (→ `[+ New Agent]`), then exit. `Tab`, `i` (vim), or
  any printable key returns to the input.

---

## Peek panel

Selecting an agent row shows the **peek panel** in place of the dispatch box.
With no row selected (`[+ New Agent]`, or after `Esc`), the dispatch box
returns. Select a row to talk to an existing agent; deselect to start a new
one.

Top to bottom: header (**last response type** — `Thinking` / `Thought` /
`Response` / `Edit` / `Read` / `Bash` / … — and **time**), the most recent
response (word-wrapped, up to ~3 rows; `…` when truncated), and a live
`❯ reply` input.

The selected agent's **model**, **Behavior**, and **Permission** are shown on
the panel's **bottom border** (bottom-right), in the same
`model | behavior | permission` order as the session prompt. This holds in
question / approval modes too, so the execution protocol and approval policy
remain visible while you answer. (The
dashboard list rows no longer repeat the model or an always-approve badge,
keeping the list compact.)

The Dashboard has no separate configuration-cycle shortcuts. Open an existing
Agent and use the normal `Ctrl+X` leader or Slash selectors. In the new-session
dispatch input, `/model`, `/effort`, `/permission`, and `/behavior` stage
independent values for the next Agent without changing persistent defaults.

Unlike the dispatch box (which only ever spawns new sessions), the
peek's reply **talks to the selected agent**:

- **Type into `❯ reply`, then `Enter`** to send. An **idle** agent
  starts the turn immediately; a **busy** agent **queues** the message
  so it sends after the current turn finishes (the same queue/drain
  behaviour as the agent view's own prompt). `Ctrl+S` replies AND
  opens the agent's detail view; `Shift+Enter` / `Alt+Enter` insert a
  newline (multiline compose) and the reply **grows in height** to fit
  the draft (up to a cap, then it scrolls).
- With an **empty** reply, `Enter` opens the agent.
- **`↑`/`↓` move the caret within the reply** once it has content (so you
  can edit a multi-line draft). While the reply is **empty** (or
  unfocused via `Tab`), `↑`/`↓` instead **switch the selected agent** —
  the panel follows the selection cursor and refreshes live, and the
  switch clears any half-typed draft so a reply can't land on the wrong
  agent. (`Tab` to the row list to navigate agents while a draft is in
  the reply.)
- **`Esc` unselects**: it first clears a typed reply, then deselects the
  row and focuses the `[+ New Agent]` button (bringing back the
  new-session input).
- **`Tab`** toggles focus between the reply input and the row list: an
  unfocused reply dims its border and hides the caret; a printable key
  re-focuses it and starts composing.
- The reply is a **full prompt editor** (the same component as the
  dispatch box and the agent prompt): pasting multi-line text folds
  into a `[Pasted: N lines]` chip with the same preview overlay and
  expand affordances as the agent prompt (`Enter` / double-click /
  paste-again), mouse click / drag place the caret and select text,
  and the usual editing chords work (word navigation, `Ctrl+A`/`Ctrl+E`,
  `Alt+Backspace`, `Ctrl+W`/`Ctrl+U`/`Ctrl+K`, undo, Shift+arrow
  selection, `Ctrl+Shift+V` inline paste).
  Typing **`@`** opens the file-context picker rooted at the **peeked
  agent's** working directory (so `@path` resolves against the agent
  you're replying to); its dropdown floats **above** the panel and
  `↑`/`↓`/`Tab`/`Enter`/`Esc` drive it while it's open.
  Dashboard chords (`Ctrl+X` stop, `Ctrl+T` pin, `Shift+↑/↓` reorder,
  …) still win over the editor while the panel is open.
- When a **permission / ask-tool question** is pending, the `❯ reply`
  row is hidden and the options are listed instead: **`↑`/`↓` move the
  highlighted option** (marked with `▸`) and **`Enter` answers** it.
  **`1`–`9`** still answer an option directly. (While answering, the
  arrows pick options rather than switching agents.)
- The **free-text row** accepts an inline typed answer (just like the
  chat panel): the permission **"No" / reject** option ("No, reject
  (type to add feedback)") and the ask-tool **"Other"** row ("Other
  (type your own answer)"). Type on it and `Enter` sends the rejection +
  message / the free-text answer.
- This also covers the **top-level session's own** Ask tool
  (`AskUserQuestion`): its options + the "Other" row show in the peek,
  answered the same way. **Multi-question** forms are walked one question
  at a time — a `(i/N)` marker shows progress and each answer advances to
  the next, submitting on the last. (Forms with a **multi-select**
  question are left to the agent's own view — open the agent to answer
  those.)
- A **subagent's** ask-tool question does not appear in the peek: the
  panel keeps the plain `❯ reply` box. Press **`Enter`** on the subagent
  row to open its full-screen view and answer the question there.
  Subagent **permission** requests surface in the parent agent's
  permission overlay, not in the dashboard peek — peek permission
  answering covers only top-level sessions' own requests.

The panel only renders when the terminal is tall enough; on very short
terminals the dispatch box shows even with a row selected.

---

## Search / filter (`Ctrl+/`)

`Ctrl+/` toggles search mode so normal typing always dispatches. Prefix
flips from `❯` to yellow `Search:`; every keystroke live-filters the list.

- `Enter` — confirm: keep the filter and return to the dispatch prompt.
- `Esc` or `Ctrl+/` — cancel: clear the filter and exit search.
- `↑` / `↓` — navigate filtered rows.

Prefixes (only inside search mode):

- `a:<name>` — Agent label (case-insensitive substring).
- `s:<state>` — row state: `working`, `idle`, `completed`, `failed`,
  `needs-input`, `blocked` and synonyms (`busy`/`running`/`done`/etc.).
- `#<text>` — substring match on `#<text>` (literal `#` in labels).
- anything else — substring over label + working dir.

---

## Persistence

Per-user dashboard preferences live under `[dashboard]` in
`~/.grow/config.toml`:

```toml
[dashboard]
enabled = true
grouping = "state"   # or "directory"
pinned   = ["top:<session_id>", "sub:<parent_session_id>:<child_session_id>"]
reorder  = ["top:<session_id>"]
```

Pinned/reorder entries are keyed by **session id**, not by the
per-process `AgentId(usize)`, so they survive restarts and don't
attach to whatever agent happens to share the old slot number.

Set `GROW_AGENT_DASHBOARD=0` to force-disable the feature for a single
pager invocation; the slash command and CLI subcommand will print a
friendly toast.

---

## Phase 4 (out of scope for v1)

The current dashboard lists only agents owned by **this** pager
process. The plan's Phase 4 ("supervisor / `grow --bg`") would list
sessions that survive pager exit — that's a separate roadmap and not
shipped yet.
