# Subagents

Subagents are independent child sessions for bounded delegated work. Each child owns its context window, model loop, tool runtime, permission state, and durable Timeline. The parent receives a terminal result only after the child lifecycle has been committed.

Subagents are enabled by default.

---

## Agent definitions

An Agent definition is the only mechanism for defining a child role. It owns the role prompt, authored tool ceiling, default capability mode, and MCP inheritance policy. Definitions are Markdown files under `.grow/agents/**/*.md` or `~/.grow/agents/**/*.md`.

The session owns mutable runtime state such as model selection, Behavior, permissions, and live grants. Grow does not layer a second behavioral-overlay system on top of an Agent.

Open `/config-agents` to inspect or edit discovered definitions.

---

## Disabling subagents

```bash
export GROW_SUBAGENTS=0
```

```toml
[subagents]
enabled = false
```

---

## Lifecycle and persistence

When the parent calls `spawn_subagent`, Grow:

1. validates and durably links an eligible TODO in the direct parent's current scope;
2. commits the spawn fact to the parent Timeline;
3. atomically creates the child session with a seed-source fact;
4. runs the child against its own Timeline and runtime;
5. commits the child's terminal result;
6. closes the parent spawn by referencing that exact child event.

Successful non-empty output is stored once as an immutable content-addressed artifact. There is no parallel metadata or output-file lifecycle.

After restart, Grow reconstructs child rows and terminal state from the parent and child Timeline facts. After a confirmed session-tree suspension, resuming the root also restores its unfinished descendants under their original identities and limits. This differs from `resume_from`, which creates a new child from a terminal lifecycle. A broken link, missing child, or mismatched checkpoint fails closed instead of selecting another transcript heuristically. See [session suspension](17-sessions.md#suspension-and-recovery).

---

## Local session coordination

Separate primary Grow sessions can ask each other a bounded question without creating a child session or interrupting either foreground turn. `list_active_sessions` lists other live primary sessions owned by the same OS user and using the same `GROW_HOME`; `ask_session` takes one returned `session_id` and a concise question. ACP clients can use the equivalent `grow/coordination/list`, `grow/coordination/ask`, and `grow/coordination/cancel` extension methods.

Discovery is local-only. Each Grow process publishes an owner-private manifest containing a fresh process incarnation, endpoint, session snapshot, and expiring heartbeat lease. Stale manifests are reclaimed, duplicate session identities are hidden, and reconnect accepts only the current incarnation. No TCP listener or MCP server is involved.

The target answers from a frozen copy of its committed conversation through one tool-free Sideband request. It cannot run tools, edit files, resume a Workflow, or promise follow-up work. Same-workspace questions proceed directly; a different canonical workspace requires an online target UI to approve that inquiry once. Rejection, cancellation, timeout, target loss, and transport failure return typed terminal outcomes and do not become ordinary user prompts or foreground turns. Both sessions retain durable audit facts. A successful answer reaches the calling Agent through its tool result; the answering Agent receives the attributed question and answer as passive context at its next admitted sampling boundary. This does not interrupt its work, start an idle turn, or grant authority.

This facility is for coordination between already-running primary sessions. Use `spawn_subagent` when one session should own delegated execution and return a task result.

---

## Questions and interventions

The behavior contract is maintained in [local-coordination](../../../../../openspec/specs/local-coordination/spec.md).

A child can use `ask_parent` to ask its immediate delegating agent for clarification. A primary agent can use `ask_subagent` with a directly-owned running child's ID. Both use the existing asynchronous, tool-free Sideband inquiry: the caller receives the answer as a tool result, while the target's foreground task continues. A child question produces one correlated inquiry row in the main-agent view, updated as it runs and completes. Delegated worktrees do not require the cross-workspace peer approval flow. Cross-session discovery and inquiry remain primary-only.

To change a running child's instructions, the primary agent uses `send_subagent_message` with `subagent_id`, `message` and an explicit `interrupt` boolean. `false` queues the attributed message for the next step's sampling. `true` safely preempts the model request or interruptible wait before resampling; tools with non-interruptible side effects finish safely. Success means the target durably received the message, not that it finished the task. A receipt that races terminal closure is rejected rather than silently accepted and lost. Parent messages do not grant human authorization or expand child permissions.

Questions do not enter the target foreground conversation; interventions do. Children cannot send intervention messages upward or contact unrelated sessions. Received interventions use Timeline notification receipt/consumption and survive restoration without duplicate delivery. An interrupted inquiry is closed during restoration rather than automatically reissuing the model request.

To change a running child's sampling, the parent uses `set_subagent_model` with that child's `subagent_id` and `model`, `effort`, or both. The tool returns `queued` after the child actor accepts the selection; it takes effect at the child's closed Step boundary without restarting the current request. The parent's selection and the child's execution permissions stay unchanged. This controls only active directly delegated children; it cannot select another primary session or a grandchild, restore a stopped task, or override a Workflow route. See [model control](11-custom-models.md#agent-control-of-a-running-child) for field defaults and errors.

---

## Built-in Agent types

The `subagent_type` argument selects the child definition:

| Type | Purpose |
| --- | --- |
| `general-purpose` | General implementation and investigation work. |
| `explore` | Read-focused codebase investigation. It can request execution when the definition allows it, but cannot request edits. |
| `browser-use` | Web browsing with the capabilities available in the session. |

Project and user definitions can add new types. Project definitions can shadow a built-in ID; a user-level definition with the same ID does not replace a built-in. Nested files use their relative path without the extension as the ID, such as `review/backend`.

---

## Spawning a child

`spawn_subagent` accepts:

| Parameter | Meaning |
| --- | --- |
| `prompt` | Complete delegated task. |
| `description` | Short task label. |
| `subagent_type` | Agent definition; defaults to `general-purpose`. |
| `background` | Return immediately with a child ID (defaults to true). |
| `capability_mode` | Initial grant: `read-only`, `read-write`, `execute`, or `all`. |
| `isolation` | `none` or an isolated git `worktree`. |
| `resume_from` | Continue a completed child by ID. |
| `cwd` | Child working directory; mutually exclusive with worktree isolation. |
| `model` | Optional explicit model selector. Use an exact configured catalog ID from the Task tool's offered model list; a provider-facing wire-model name alone does not select a route. |
| `effort` | Optional reasoning effort for the selected model, or the model resolved by default. It may be set with or without `model`, subject to the model's offered effort levels. Fresh calls use the existing default when omitted; resume calls retain the source's last durable effort when omitted. |
| `parent_step_id` | Required TODO identity in the direct parent's current scope. The item must be pending or in progress and not removed. |
| `required_deliverable` | Require explicit parent review of the linked result. Both optional and required deliveries need `parent_step_id`. |

The separate `effort` parameter is available from Grow 2.6.1. If an Agent reports that it is absent, check the actual tool schema in that session's provider request and the version of the process serving the session. Installing a newer binary does not replace the tool runtime of an already running older process; `grow --version` in another shell may resolve to a different installed binary. Use the newer process when creating or restoring the session's tool runtime.

Observe children with three separate tools:

- `list_subagent({"scope":"active"})` discovers visible unfinished or unknown children, including initialization, suspension and finalization. Use `"all"` to include authorized history. The list contains basic identity/state facts, without tool calls or answers.
- `get_subagent_status({"subagent_ids":["id"]})` reads non-blocking details: status, duration, call/turn counts and available usage. Unavailable values are `null`.
- `wait_subagent({"subagent_ids":["id"],"timeout_ms":30000})` waits and retrieves final results. The default timeout is 30 seconds and the advertised cap applies. Already finished children return immediately. Each call reports its end reason and actual wait time. Failed or cancelled children are finished without being successful.

Use `kill_command_or_subagent(task_id)` to cancel a child. This task control can address a child without an attached Terminal resource. `kill_terminal_command(task_id)` is limited to background terminal commands and monitors; it does not cancel children.

All three tools use the current source's authorization. Missing runtime entries do not prove cleanup; canonical history can retain the child's identity and verified result. Lists and status queries leave completion notifications available. Background command output uses `get_command_output` with `task_ids`. See the [observation contract](../../../../../openspec/specs/session-timeline/spec.md#requirement-subagent-tools-separate-enumeration-detail-and-completion-waiting).

Continue useful independent work after delegation. When the next step requires the child's result and no independent work remains, use `wait_subagent` on the original child ID. The background-start receipt shows the current tool and parameter names. Do not create sleep commands, polling loops, or extra tasks solely to wait. Include multiple IDs only when the next step requires all their results.

A timeout or steering message can end the wait while the child continues. Check the returned status and result; wait completion does not establish child completion or parent acceptance. The existing View shows the output wait without creating another Task row, and the underlying task retains its owner and identity. See the [primary coordination guidance](../../../../../openspec/specs/local-coordination/spec.md#requirement-primary-guidance-advances-independently-ready-work).

### Resume semantics

`resume_from` creates a new child from a terminal canonical lifecycle, including a canonically cancelled child. It retains the source transcript and existing stable system head; the current Agent role and tool runtime are rebuilt under the current constraints. In-memory permits and transport incarnations are never copied. The original runtime must finish retiring before the new child can start.

Optional `model` and `effort` independently override the source's last durable selection, including changes made while the source was running. Omission or JSON null retains that source field; `effort: "none"` selects an explicit effort. If only `model` is supplied, the source effort is retained and the request fails if the new model does not offer it. Supplying a new model selects its current configured route; without a model override, the source transport must still be available. Invalid overrides fail rather than starting a fresh child or substituting defaults. The source history remains unchanged.

For example, `{"resume_from":"child-id","parent_step_id":"review-step","description":"Continue review","prompt":"Check the remaining cases.","model":"provider/model","effort":"high"}` resumes on the chosen model and effort, linked to the currently eligible `review-step`. The source must belong to the authorized parent lineage and use the same Agent type. These overrides apply to ordinary `resume_from`; recovery of a suspended child under its original identity keeps the checkpoint's route and limits.

The parent can use [`set_subagent_model`](11-custom-models.md#agent-control-of-a-running-child) while the child is running. A later ordinary resume that omits model or effort inherits the child's last durable selection for each omitted field.

---

## Local TODOs and delegated work

Each session owns its durable local TODO plan. Create an eligible parent TODO before ordinary `spawn_subagent` or `resume_from`; Grow confirms that binding before starting the child. A child can split its work into its own steps. Linking a parent item does not copy the parent's list or let the parent overwrite the child's list. Several children may support one parent step. A simple task that does not delegate can proceed without a TODO or Goal.

Use `todo_read` to inspect plans without changing them. `target: "self"` is the default; `target: "parent"` reads the immediate delegating parent's plan. An optional `step_id` selects the item and its related obligations. Follow `next_cursor` for further pages and restart the read if the snapshot becomes stale. The tool reports an unavailable owner separately from an available owner with no plan. Each child request also receives a bounded summary identifying its current parent item and position in the overall plan, including the first request after a resume. This summary is request context and does not become part of the authored task prompt or chat history.

An actual change to a linked parent's content, requirements, priority or non-completion status creates objective-change guidance for its unfinished children. The child reads the current TODO, preserves applicable completed work and adjusts affected steps, or records a reasoned no-change decision. Completion alone, identical writes, sorting and receipt updates stay quiet. Receiving or consuming guidance does not acknowledge handling it.

Removing or cancelling the parent item tells each linked unfinished child to stop advancing that objective, preserve work, settle outstanding operations and report completed work, remaining work and uncertainty. Reopening the TODO cannot revoke that instruction for the original execution; new work requires a new delegation. `HandleReplans` acknowledges this instruction with a stopping outcome. Commands, descendant tasks, results and required reviews retain their actual owners until they settle. Use the existing explicit task control for immediate cancellation.

Grow generates these controls from committed planning facts and safely interrupts sampling or an interruptible wait after durable receipt. The internal control is hidden from ordinary coordination chat notices in live and restored history, while its receipt, model consumption and handling remain inspectable through the plan. Ordinary intervention messages retain their visible provenance even if their text copies the control wording. Changes reach a grandchild through an actual edit to its immediate parent's linked step.

The Agent can register structured waits for real task, command, question, or permission identities with any/all wake conditions. A yielded child remains unfinished and continues under the same owner and limits when the dependency changes; prose such as “waiting for tests” alone does not register a wait. Failed or interrupted dependencies wake a decision, not a claim of success.

For a required deliverable, child completion, delivery, and parent acceptance are separate. The parent reviews the actual result and validation or explicitly releases the obligation with a reason. Marking a TODO complete does not accept the result or merge a worktree. An older result may still satisfy revised requirements after explicit review.

These rules apply to ordinary tasks and Goal work; local TODOs do not approve a Plan, change permissions, or create a Goal. See the [task-planning contract](../../../../../openspec/specs/task-planning/spec.md).

---

## Capability model

The Agent definition establishes a hard-eligible exact-tool ceiling. The capability mode establishes immutable initial RWX inside that ceiling:

| Mode | Read | Write | Execute |
| --- | --- | --- | --- |
| `read-only` | Yes | No | No |
| `read-write` | Yes | Yes | No |
| `execute` | Yes | No | Yes |
| `all` | Yes | Yes | Yes |

When neither the `Task` call nor the Agent definition selects a mode, initial RWX is `read-write`. For a nested child, Grow intersects that requested/default mode with the immediate parent's immutable delegation ceiling before creating any child-side resources. Thus an `execute` child requested by a `read-write` parent starts `read-only`. Parent approval history never enlarges what it may delegate.

Finalized tools remain truthfully discoverable. Each exact call is projected onto the closed RWX lattice:

- A hard-eligible call covered by initial RWX uses the ordinary fast path.
- A hard-eligible call outside initial RWX is locked. Invoke it directly; Ask/Auto decides that frozen call.
- A call outside hard eligibility is rejected before permission UI or model judgment.

An allow decision signs a one-shot permit bound to the call id, exact native/MCP identity, canonical arguments, cwd, projected RWX, child epoch, and MCP transport generation. Dispatch consumes it once and revalidates every binding. Approval never changes the child's RWX, unlocks a server for later calls, or affects siblings and descendants.

Configured permission policy, hooks, protected edits, interactive operations, Behavior/Goal ownership, and depth limits remain authoritative regardless of initial RWX or an allow decision.

---

## Permission mode

```toml
[subagents]
permission_mode = "auto"       # auto | ask | always-approve
classifier_input = "context"   # context | request_only
```

- `auto` classifies a locked exact call against the configured primary-context view. Calls already inside initial RWX do not invoke the classifier unless a secondary shell-risk signal escalates them.
- `ask` routes a locked exact call to the real child-session approval UI with only allow-once and reject-once.
- `always-approve` still applies configured permission-policy clamps.

The child mode is resolved when the child is created and never follows later primary-session mode changes. A missing internal child route resolves to `auto`; it is not interpreted as permission inheritance.

Classifier input and verdicts are ephemeral and never become primary model history. Invalid output, timeout exhaustion, or a non-retryable provider failure denies only that exact call.

---

## MCP inheritance

Children inherit the parent's connected and enabled MCP catalog by default. Inheritance establishes exact-tool eligibility, not session authorization. `search_tool` labels eligible results `call_bound`; call `use_tool` with the returned schema. If the server's projected access is outside initial RWX, that exact invocation enters Ask/Auto.

The inherited ceiling stays live: removing a parent server or hiding a tool immediately makes it ineligible for descendants.

Cold restoration waits for the direct parent's initial MCP discovery before binding surviving birth members to new connections. The original durable membership, current Agent profile and parent ceiling still bound the restored grant; newly configured services cannot enlarge it. Missing birth membership blocks execution, while a known empty set stays empty. See the [cold-restore contract](../../../../../openspec/specs/tool-authorization/spec.md#requirement-cold-child-grants-wait-for-initial-parent-mcp-discovery).

Agent frontmatter controls inheritance:

| Value | Effect |
| --- | --- |
| `all` | Inherit every parent-connected server. |
| `none` | Inherit none. |
| `named: [server, …]` | Inherit only listed servers. |
| `except: [server, …]` | Inherit all except listed servers. |

```yaml
---
name: research-only
description: Research with selected MCP tools
tools: search_tool, use_tool, Read
mcpInheritance:
  except:
    - internal-tools
---
```

Plugin Agents use the same rule. They cannot declare child-only MCP servers or hooks in Agent frontmatter; trusted plugin MCP configuration attaches to the parent session catalog.

---

## Worktree isolation

Set `isolation: worktree` for editing tasks that must not share the parent's working tree. Grow creates a managed worktree, reports its path in the child result, and exposes an explicit apply operation through `grow/git/worktree/*`.

The child does not silently merge its changes into the parent workspace.

---

## Configuration

Toggle Agent types or route them to a configured model:

```toml
[subagents.toggle]
explore = true
plan = false

[subagents.models]
explore = "deepseek/deepseek-chat"
```

Without an override, a child inherits the parent's model.

Define custom roles in Agent Markdown:

```markdown
---
name: researcher
description: Evidence-driven repository investigator
toolPreset: explore
capabilityMode: read-only
---
Investigate the delegated question and report concrete file-level evidence.
```

---

## TUI and debugging

- `Ctrl+G` toggles the tasks pane for active and completed children and background commands.
- `/config-agents` opens the Agent-definition catalog.
- Enter on a child lifecycle row opens its framed transcript.
- `q`, `Esc`, or the close button returns to the parent.

The parent scrollback shows spawn, progress, permission audit, and terminal rows. The child frame shows the complete child transcript, thinking blocks, tool calls, live activity, elapsed time, model, and resume/fork state.

For event-level debugging, open the Trajectory page exposed by the local session debug server. It reads the same durable Timeline projection used for recovery, with filters for layer, actor, class, producer, visibility, text, and cursor range.

---

## Depth limits

`[subagents].max_depth` limits recursive spawning. At the boundary the spawn tool is removed from the child's eligible toolset. A call-bound permit cannot restore it or bypass Goal/Workflow ownership.

Use subagents for independent investigation, implementation, focused testing, and review. Keep tightly interactive or trivially small work in the parent session.
