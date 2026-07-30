# Plan Mode & Permissions

Grow asks before doing anything risky — and can plan before it codes.

## Permissions

When Grow wants to run a risky command or edit a file, it pauses and asks:
allow once, always allow that kind of action, or deny.

Reading is always free: file reads, searches, and safe read-only commands
(`ls`, `git status`, `grep`, …) never prompt. Chained commands are
checked piece by piece — `ls && rm -rf tmp` still prompts for the `rm`.

Trust the session? `/always-approve` selects that policy directly; `Ctrl+X`,
then `P` opens the Permission picker.

## Plan mode

For bigger or more ambiguous tasks, use **plan mode**: Grow explores the
codebase read-only, designs an approach, and presents a plan you approve
*before* any code is written.

- **`Ctrl+X`, then `B`** opens the Behavior picker; select **Plan**.
- **`Ctrl+X`, then `P`** opens the independent Permission picker.
- **`Ctrl+R`** remains prompt redo and never changes Behavior or Permission.
- **`/plan`** enters plan mode directly; `/plan <task>` plans that task in
  one step.

When the plan is ready: `a` approves, `c` comments on a specific line,
`s` requests changes — Grow iterates until you're happy, then implements.

A good habit: Plan for "agree on the whole approach first", Clarify when
missing details should stay with you, and Workflow for autonomous dynamic
sub-planning.

## Long-running commands

A build or test run hogging the turn? **`Ctrl+B`** sends it to the
background — Grow keeps working and you're notified when it finishes
(`Ctrl+G` shows the tasks pane).

*Go deeper: `/docs Plan Mode` or `/docs Permissions and Safety`*
