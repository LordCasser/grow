# Plan Mode

Plan mode is a structured planning behavior: the agent investigates available facts and produces a decision-ready approach before implementation. Use it for work with genuine ambiguity, where getting your input first prevents significant rework.

---

## What Plan Mode Does

When plan mode is active, the agent:

1. Investigates the available environment, constraints, and existing design
2. Builds the complete plan in context
3. May use `ask_user_question` to clarify specific questions
4. Calls `plan_control(action="submit", plan=...)` to present the plan for your approval

Plan is a human-approval protocol, not a permission preset. Before approval, workspace mutation is rejected in every permission mode, including shell and MCP execution paths that may mutate state. Read, search, web, and question tools still pass through their normal policy and permission checks. After approval, the frozen plan becomes the execution contract and normal mutation capabilities reopen.

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

- **`/plan`** -- Enter plan mode. Plan mode activates when you send your next prompt. Run `/plan <description>` to enter plan mode and start a turn with that description in one step.
- **Ctrl+X, then B** -- Open the Behavior picker and select Plan. The same picker selects Normal, Clarify, Static Workflow, Deep Research, and Goal when those Behaviors are available.

Permission is selected independently through `Ctrl+X P` or `/permission`;
`Ctrl+R` remains the prompt editor's redo shortcut.

After a plan exists, run **`/view-plan`** (aliases `/show-plan`, `/plan-view`) to reopen its saved preview.

---

## The Plan Artifact

The complete plan is passed to `plan_control`. Before approval opens, Grow atomically persists the candidate at `plan.md`. Approval freezes the exact accepted version at `approved_plan.md` in the same session directory. Execution reminders inject that frozen artifact as a contract; later material changes require `action="amend"` and another approval.

This artifact belongs to the session control plane. It is not exposed as a model-editable workspace target and does not grant the Agent arbitrary file-write access.

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
| `q`      | Quit plan -- abandon the plan without approving and turn plan mode off.                              |

Press `Tab` to move focus between the plan preview and the prompt.

### Providing Feedback

The approval view has three focus states:

- **Preview**: Scroll the plan and select lines to comment on.
- **Commenting**: Add an inline comment to the selected line range (press `c`, or `Enter` on a line).
- **Prompt**: Type freeform revision notes.

Press `Tab` to switch between the preview and the prompt. When you send feedback -- inline comments, freeform notes, or both -- the agent receives it and revises the plan. Plan mode stays active so you can iterate.

### Leaving the Approval View

Press `Esc` to return focus from the prompt to the plan preview. To dismiss the approval without approving or sending feedback, press `q` to quit the plan. Quitting abandons the proposed plan and turns plan mode off.

---

## Plan Mode Lifecycle

The user-facing Plan protocol has these phases:

| Phase | Description |
| --- | --- |
| `Drafting` | Investigate and form a complete plan; workspace mutation is blocked. |
| `AwaitingApproval` | The submitted candidate is frozen for user review; mutation remains blocked. |
| `Executing` | The approved version is injected as the execution contract; mutation is allowed by normal policy. |
| `Amending` | A material deviation has stopped execution and awaits approval of a complete replacement plan. |
| `Completed/Cancelled` | The lifecycle is cleared and Behavior returns to Normal. |

Transitions:

`submit` moves Drafting to AwaitingApproval. Approval moves to Executing. `amend` moves Executing to Amending and approval returns to Executing with a new frozen version. `complete` and `cancel` return to Normal. Plan state and the outstanding approval survive process restarts.

---

## Edits During Plan Mode

During Drafting, AwaitingApproval, and Amending, **all workspace mutation is rejected before permission evaluation**, including attempts to edit the session artifact path. Static Workflow is not advertised and stale Static Workflow calls are rejected. During Executing, edits are allowed only through the ordinary intersection of registered tools, Agent policy, permissions, and the Behavior gate.

MCP servers declared with `[mcp_servers.<name>] tool_scope = "read"` are the one exception to the mutation gate: every tool of that server may pass the gate, but still goes through the normal permission policy; all `write` or unclassified MCP tools remain rejected. The scope is server-wide — split servers that mix read and write tools, or leave the conservative `write` default.

This enforcement is independent of the permission mode:

- **Always-approve stays armed as an independent permission policy**, but it cannot bypass a non-executing Plan phase's mutation gate.
- Behavior belongs only to the primary Agent. Child Agents receive an explicit role, task, and capability boundary; they neither inherit Plan nor select another Behavior.

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
- Evidence-heavy research tasks that require a terminal report (use Deep Research)
