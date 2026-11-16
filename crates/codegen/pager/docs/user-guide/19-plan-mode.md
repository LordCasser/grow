# Plan Mode

Plan mode is a structured planning behavior: the agent investigates available facts and produces a decision-ready approach before implementation. Use it for work with genuine ambiguity, where getting your input first prevents significant rework.

---

## What Plan Mode Does

When plan mode is active, the agent:

1. Investigates the available environment, constraints, and existing design
2. Builds the complete plan in context
3. May use `ask_user_question` to clarify specific questions
4. Calls `plan_control(action="submit", plan=...)` to present the plan for your approval

Plan is a human-approval protocol, not a permission preset. Tool calls use the same capability, permission, sandbox, and path checks as Normal. During Drafting, AwaitingApproval, and Amending, the agent may read, search, run terminal checks, and perform limited recoverable experiments, but only to form, verify, or revise the plan. Allowing a tool call does not approve the plan. The agent must not implement the planned product change, including through a script, file edit, MCP tool, subagent, another session, or Workflow. That instruction guides the agent; it is not a sandbox that blocks every command. After approval, Executing implements only the frozen scope under ordinary permissions. Work outside that scope needs an amendment and another approval.

---

## How to Enter Plan Mode

Plan is selected explicitly by the user. The Agent cannot enter it through a
tool call: `plan_control` is available only after Plan is selected and controls
the approval lifecycle inside that Behavior.

**Good triggers for plan mode:**

- "Add user authentication to the app" -- genuinely ambiguous (session vs JWT, token storage, middleware structure)
- "Redesign the data pipeline" -- major restructuring where the wrong approach wastes significant effort
- "Add caching to the API" -- multiple reasonable approaches (Redis vs in-memory vs file-based)
- "Add real-time updates" -- architectural decision (WebSockets vs SSE vs polling)

**Not appropriate for plan mode:**

- "Add a delete button to the user profile" -- clear implementation path
- "Fix the typo in the README" -- straightforward
- "Update the error handling in the API" -- start working, ask specific questions if needed
- "Can we work on the search feature?" -- user wants to get started, not plan

You can enter plan mode yourself in two ways:

- **`/plan`** -- Select Plan for the current session. On the dashboard, this stages Plan for the next session you start. Run `/plan <description>` to enter plan mode and start a turn with that description after the mode switch succeeds.
- **Ctrl+X, then B** -- Open the Behavior picker and select Plan. The same picker selects Normal, Clarify, Workflow, and Goal when those Behaviors are available.

Permission is selected independently through `Ctrl+X P` or `/permission`;
`Ctrl+R` remains the prompt editor's redo shortcut.

After a plan exists, run **`/view-plan`** to reopen its saved preview.

The top status bar shows **`[Plan: Drafting]`**, **`[Plan: Review]`**,
**`[Plan: Executing]`**, or **`[Plan: Amending]`** while Plan is active.
Click it to open the complete plan in the plan detail view. The entry remains
available after approval and after reconnecting, even before the plan body has
been loaded into the UI. Before the first submission, opening it reports that
no plan artifact is available yet.

The **TODO progress badge and task list remain separate** in Plan mode. They
track actual execution steps; opening the Plan details does not replace or
change them. Switching Agents preserves the session's TODO list and its tool.
See [Plan status and details](../../../../../openspec/specs/client-surfaces/spec.md#requirement-active-plan-has-an-inspectable-top-status-entry)
and [planning across Agent replacement](../../../../../openspec/specs/task-planning/spec.md#requirement-agent-replacement-preserves-session-planning-access).

When Plan completes, the main conversation gets a brief normal summary, while
the complete original report is saved in the **Result report** tab of Plan
details. The **Plan** tab keeps the approved plan. Click either tab, or use
`Tab` / `Shift+Tab`, to switch between them. Completed details are read-only.
The **`[Plan: Complete]`** entry remains available after reconnecting until your
next successful manual Behavior selection, including selecting Normal again.
See [completed Plan details](../../../../../openspec/specs/client-surfaces/spec.md#requirement-completed-plan-details-separate-the-full-report-from-the-summary).

---

## The Plan Artifact

The complete plan is passed to `plan_control`. Before approval opens, Grow persists an immutable body at `artifacts/plan/<blake3-hash>.md` inside the session directory and commits its submission identity, revision, and hash. Approval binds that exact candidate; execution reminders use the approved snapshot as a contract. Later material changes require `action="amend"` and another approval.

This artifact belongs to the session control plane. The agent submits plan text through `plan_control`; the artifact is not a free-form edit target, and a recoverable experiment does not grant arbitrary file-write access.

The plan file contains:

- A **Context** section explaining why the change is being made
- The recommended approach (not every alternative)
- The paths of critical files to modify
- Existing functions and utilities to reuse, with their file paths
- A verification section describing how to test the changes end to end

---

## Plan Approval

When the agent finishes planning, it calls `plan_control(action="submit", plan=...)`. Grow validates that the submitted plan is non-empty, atomically persists it, and then opens a scrollable preview with an action bar along the bottom.

An empty or whitespace-only plan is rejected and Plan remains active. If persistence fails, approval does not open and the Agent receives the failure so it can retry.

### Reviewing the Plan

Scroll the plan with the arrow keys or `j`/`k`. The action bar shows these shortcuts:

| Shortcut | Action                                                                                               |
| -------- | ---------------------------------------------------------------------------------------------------- |
| `a`      | Approve the plan and start building. With pending comments, this reads `approve w/ comments` and sends them alongside the approval. |
| `s`      | Request changes. Focus moves to the prompt so you can type revision notes; press `Enter` to send them. |
| `c`      | Comment on the selected line or line range.                                                          |
| `y`      | Copy the full plan to the clipboard.                                                                 |
| `l`      | Keep the candidate for later review without approving or starting implementation.                    |
| `q`      | Abandon the plan without approving and turn plan mode off.                                           |

Press `Tab` to move focus between the plan preview and the prompt.

### Providing Feedback

The approval view has three focus states:

- **Preview**: Scroll the plan and select lines to comment on.
- **Commenting**: Add an inline comment to the selected line range (press `c`, or `Enter` on a line).
- **Prompt**: Type freeform revision notes.

Press `Tab` to switch between the preview and the prompt. When you send feedback -- inline comments, freeform notes, or both -- the agent receives it and revises the plan. Plan mode stays active so you can iterate.

An empty `Enter` in the revision prompt does not approve or submit anything.
Use `a` on an empty prompt without pending comments to approve, or return to
Preview with `Tab` to approve with comments and see all review actions. Minimal also displays the editor while you write a line
comment. In a narrow preview, the action bar wraps onto multiple rows.

### Leaving the Approval View

In the preview, `Esc` first clears transient search or selection, then collapses the page; the close button also only collapses it. From the revision prompt, `Esc` collapses any open preview. Use `Tab` to reopen it. Neither action approves, abandons, or defers the plan, and the approval remains pending. While editing a line comment, `Esc` cancels that edit and restores your previous revision draft.

Press `l` in the approval preview to keep the candidate for later. Once that decision is durably confirmed, Grow releases the approval wait without starting implementation. Run `/view-plan` to read it again, then press `r` in the retained preview to request a new review. Viewing alone never approves it. If another Behavior owns the session, its admission rules still apply; the saved plan does not take over automatically. Press `q` in the approval preview only when you intend to abandon it.

Accepted decisions close the confirmation view automatically when the server acknowledges that exact decision. If a decision remains unconfirmed, use `r` to check its status rather than submitting another decision. Approval and comments remain bound to the reviewed session and submission. If the candidate changes, review the new one; old comments remain unsent drafts rather than being applied to a different plan.

Outside a live approval, comments sent from an ordinary Plan preview are
ordinary chat messages. If the agent is busy, they enter the normal message
queue; sending them does not itself return the Plan to revision or approve it.
For a retained candidate, use `r` to open a formal review first. Completed
Plan and Result report tabs remain read-only.

See the [retained-plan contract](../../../../../openspec/specs/behavior-goal/spec.md#requirement-retained-plans-remain-under-the-existing-session-owner).

---

## Plan Mode Lifecycle

The user-facing Plan protocol has these phases:

| Phase | Description |
| --- | --- |
| `Drafting` | Investigate and form a complete plan. Tools follow normal permissions and serve planning only. |
| `AwaitingApproval` | The submitted candidate is frozen. The agent must not execute, edit, or resubmit it. |
| `Executing` | The approved version is the execution contract. Normal tool permissions apply inside that scope. |
| `Amending` | A complete replacement is being revised, or is frozen for review. Unapproved scope is not implemented. |
| `Completed/Cancelled` | The lifecycle is cleared and Behavior returns to Normal. |

Transitions:

`submit` moves Drafting to AwaitingApproval. Approval moves to Executing. `amend` moves Executing to Amending and approval returns to Executing with a new frozen version. `complete` and `cancel` return to Normal. Plan state and the outstanding approval survive process restarts. A retained candidate has its own review status; deferring it does not add a new execution phase or approve implementation. Copying plan history to another session does not copy approval: the target needs a new submission and review. State import rejects a source that still carries a reviewable or unresolved Plan execution relationship; closed history remains importable.

---

## Edits During Plan Mode

Planning phases do not add a second mutation gate before permission evaluation. Reads, terminal checks, edits, MCP, and subagents are admitted or denied by the ordinary tool, Agent, sandbox, and permission policy. The agent must still keep that work recoverable and limited to forming, verifying, or revising the plan. It must not deliver the product change, discard your work, or commit or deploy it as investigation. Workflow remains unavailable as a way around plan approval.

During Executing, the same ordinary policy applies, limited to the approved scope. Always-approve and a one-time tool allow do not approve a plan or widen an approved scope. Behavior belongs only to the primary Agent. Child Agents receive an explicit role, task, and capability boundary; delegated planning research carries the same limit, and children neither inherit Plan nor select another Behavior.

The prompt status line always shows Behavior between model and permission. Plan includes its current phase, and the permission indicator remains visible because the two dimensions are independent.

---

## Plan Mode and Compaction

When `/compact` runs during an active plan mode session, the plan mode state is preserved. The compacted context includes a reminder that plan mode is active, so the agent continues planning after compaction.

---

## When Plan Mode is Appropriate

**Use plan mode for:**

- Tasks with significant architectural ambiguity (multiple reasonable approaches)
- Unclear requirements that need exploration before implementation
- High-impact restructuring where the wrong approach wastes significant effort

**Skip plan mode for:**

- Tasks with a clear implementation path
- Bug fixes where the fix is obvious once you understand the bug
- Adding features that follow existing conventions
- Straightforward modifications (renaming, formatting, adding tests)
