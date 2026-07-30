# Plan Mode

Plan mode is a structured planning behavior: the agent investigates available facts and produces a decision-ready approach before implementation. Use it for work with genuine ambiguity, where getting your input first prevents significant rework.

---

## What Plan Mode Does

When plan mode is active, the agent:

1. Investigates the available environment, constraints, and existing design
2. Builds the complete plan in context
3. May use `ask_user_question` to clarify specific questions
4. Calls `exit_plan_mode` to present the plan for your approval

Plan mode is a planning behavior, not a tool preset or a strict no-side-effect sandbox. Ordinary file-edit calls are rejected outright in every permission mode, including always-approve. Other tools already authorized for the Agent remain available for investigation and verification and continue through their normal permission flow. The completed plan is submitted through `exit_plan_mode` and stored as session-owned state before approval.

---

## How to Enter Plan Mode

### Agent-Initiated Entry

The agent enters plan mode when it determines a task has genuine ambiguity. It calls the `enter_plan_mode` tool, which requires your approval before plan mode activates. If you decline, the agent stays in normal mode.

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

### User-Initiated Entry

You can enter plan mode yourself in two ways:

- **`/plan`** -- Enter plan mode. Plan mode activates when you send your next prompt. Run `/plan <description>` to enter plan mode and start a turn with that description in one step.
- **Ctrl+R** -- Cycle the session mode: Normal, then Plan, then Auto (when enabled), then Always-approve, then back to Normal. From Normal, a single press lands on Plan.

After a plan exists, run **`/view-plan`** (aliases `/show-plan`, `/plan-view`) to reopen its saved preview.

---

## The Plan Artifact

The complete plan is passed in the `plan` argument to `exit_plan_mode`. Before approval opens, Grow atomically persists it as a session-owned artifact at `plan.md` inside the session directory (`~/.grow/sessions/<cwd>/<session-id>/plan.md`, where `<cwd>` is an encoded directory name, not the literal path).

This artifact belongs to the session control plane. It is not exposed as a model-editable workspace target and does not grant the Agent arbitrary file-write access.

The plan file contains:

- A **Context** section explaining why the change is being made
- The recommended approach (not every alternative)
- The paths of critical files to modify
- Existing functions and utilities to reuse, with their file paths
- A verification section describing how to test the changes end to end

---

## Plan Approval

When the agent finishes planning, it calls `exit_plan_mode(plan=...)`. Grow validates that the submitted plan is non-empty, atomically persists it, and then opens a scrollable preview with an action bar along the bottom.

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

The plan mode state machine has four states:

| State          | Description                                                    |
| -------------- | -------------------------------------------------------------- |
| `Inactive`     | Normal operating mode. No plan mode constraints.               |
| `Pending`      | Client toggled plan mode ON, but no prompt has been sent yet.  |
| `Active`       | Plan behavior is active. All ordinary file-edit calls are rejected. |
| `ExitPending`  | User toggled plan mode OFF while a turn is in-flight.          |

Transitions:

```
Inactive    --> Active   (enter_plan_mode tool called and approved -- skips Pending)
Inactive    --> Pending  (you toggle plan mode on with /plan or Ctrl+R)
Pending     --> Active   (your first prompt activates plan mode)
Active      --> Inactive (exit_plan_mode approved, or you toggle plan mode off when idle)
Active      --> ExitPending (you toggle plan mode off while a turn is in-flight)
ExitPending --> Inactive (after the turn completes)
```

Plan mode state is persisted to disk and survives process restarts. Transient states (`Pending`, `ExitPending`) are collapsed to `Inactive` on restart since they depend on in-flight interactions.

---

## Edits During Plan Mode

During active Plan behavior, **all ordinary file-edit calls are rejected before permission evaluation**, including attempts to edit the session artifact path. The Agent revises the plan in context and submits the complete version through `exit_plan_mode`.

This enforcement is independent of the permission mode:

- **Always-approve (yolo) stays armed underneath plan mode.** Non-edit tools (bash commands, reads, MCP tools) still auto-run, but file edits are blocked until you approve exiting plan mode. Once the plan is approved, always-approve resumes for implementation.
- Bash commands are not inspected for file writes — plan mode blocks the edit tools, not shell redirection.
- Behavior is selected independently for each subagent task. A parent in Plan does not change the child's role, tool allow/deny policy, or capability mode. Select `behavior: plan` for a child that must use the same Plan guidance and edit gate; otherwise its own resolved permissions apply.

The status flag shows `plan` while plan mode is active. If always-approve is enabled underneath, its flag reappears when plan mode exits.

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
- Research and exploration tasks (use subagents instead)
