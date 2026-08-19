# Prompt, Agent, Behavior, and Tool Boundaries

Grow composes an Agent session from independent layers. The layers have a fixed order and later layers may only narrow capabilities established earlier.

```text
Mandatory Core
  + Audience (primary | subagent)
  + Standard Guidance (extend only)
  + Agent Role (Markdown body)
  + Active Behavior (normal | clarify | plan | workflow | deep-research | goal)
  + Session Extensions (memory)
```

Mandatory Core contains instruction priority, action safety, tool-use rules, project-instruction scoping, output rules, and Grow client context. It is always rendered. `promptComposition: full` replaces the optional standard guidance; it does not replace Mandatory Core, Audience, an active Behavior, or Session Extensions.

Runtime facts do not belong to the system prompt. At session start, the shell renders one typed `RuntimeContextSnapshot` as a user-role message containing the visible workspace, OS, shell, local date, and optional VCS snapshot, then durably appends it to Timeline. Agent definitions cannot override its shape. Skills, project instructions, MCP catalogs, capability changes, and reminders each publish their own Timeline-backed messages instead of being copied into either the system prompt or the runtime snapshot.

Audience defines ownership boundaries: the primary Agent owns the user-facing result, the task-wide understanding needed to produce it, and the integration of delegated results, while a subagent owns only its delegated task. Primary ownership cannot be transferred wholesale: delegation may extend coverage or isolate bounded work, but the primary Agent directly examines central evidence, retains cross-cutting synthesis, and continues independent work while children run whenever useful work remains. Audience never declares a role, toolset, or Behavior.

Built-in and user-defined Agents are Markdown files with YAML frontmatter. The Markdown body is the role and use-case prompt. Tool configuration is explicit and independent:

```yaml
promptComposition: extend
toolPreset: grow-build
additionalTools: []
disallowedTools: []
subagentOnly: false
subagents:
  allow: []
  deny: []
```

`subagentOnly` is the definition-owned usage boundary. Such definitions remain
available to the Task tool but never appear in the primary Agent picker. Other
file-defined Agents must declare enough tools to read the workspace, edit or
write it, and execute verification; the picker and `/doctor` consume the same
eligibility result. The Agents settings UI only enables or disables definitions
and does not rewrite this architectural purpose.

`toolPreset` is resolved first, followed by `additionalTools`, fixed runtime injection, Agent denies and subagent policy, session clamps, depth/ownership/plugin/MCP eligibility, and ToolBridge finalization. Requestable native Execute/ReadWrite eligibility requires both an authored matching kind from `toolPreset`/`additionalTools` and a surviving implementation in the finalized bridge; runtime injection cannot silently expand that ceiling. For a subagent, `capabilityMode` no longer deletes eligible tools: it seeds the child-local current grant set. Model tool definitions and call dispatch both filter the finalized bridge through that grant set. `ToolKind`, `ToolMetadata`, registry finalization, and template name resolution retain their existing responsibilities.

The built-in Agent, tool, and session surface is Grow-native. External vendor schemas are not exposed as Agent profiles, tool presets, namespaces, ignored frontmatter fields, or session scanners. Agent files use the documented Grow schema and reject unknown keys; source provenance does not create a vendor execution mode.

Behavior is mutually-exclusive primary-session state, not part of `AgentDefinition`. The state is exactly `Normal | Clarify | Plan(phase) | Workflow | DeepResearch { run_id } | Goal`. ACP maps these to `default`, `ask`, `plan`, `workflow`, `deep_research`, and `goal`. Every mode request, including prompt metadata and slash-command shortcuts, passes through the same Behavior transition gateway. Delegated Agents receive an explicit role and task; they never inherit or select a user-facing Behavior. A Behavior never adds a tool, changes which Agent may be delegated to, changes capability mode, or bypasses permission checks.

The layers have different owners and must not be substituted for one another:

- **Behavior** is the primary Agent's protocol for advancing the user's request.
- **Role** is a delegated Agent's bounded responsibility.
- **Workflow Workspace** is session-owned control state: session drafts, one explicit Definition focus, origin/baseline/current/validated hashes, publish conflicts, and per-hash save reminders.
- **Workflow Definition** is editable content in exactly one scope (`Session | Project | User | Builtin`). Saved Definitions are derived into session drafts before Grow modifies them.
- **Workflow Run** is an immutable execution/journal snapshot owned by the workflow engine. It persists the Definition id, scope, content hash, script, and args. Leaving Workflow does not cancel an ordinary Run.
- **GoalTracker** is the persistent objective, continuation, and verification instance used while Goal Behavior is selected.

The effective call rule is:

```text
registered by Tool Preset / Registry
  ∩ Agent tool and subagent policy
  ∩ session clamp, depth, ownership, plugin trust, and MCP inheritance
  ∩ subagent current capability grants
  ∩ active Behavior gate
  ∩ session permission decision
```

The first three lines form the subagent's hard eligibility ceiling. A child sees only an eligible capability catalog, but the catalog is not authorization. `request_tool_access` may add exactly one native capability (`execute` or `read-write`) or one eligible MCP server to the current live child. A successful grant affects the next model sample and cannot restore anything removed by the hard ceiling. The dispatch gate repeats the same check so a forged tool call cannot bypass model-definition filtering. The resulting Shell, edit, or MCP invocation then passes through the ordinary permission manager as a second, independent decision.

The capability catalog is an audience/runtime layer, not role prose. The subagent audience explains the request protocol; a native system reminder lists eligible native groups and current status; MCP reminders list inherited eligible servers and their grant status. `search_tool` searches only the finalized child MCP index and annotates results that still require a server grant. The parent's model-visible MCP allowset is a live, depth-preserving authority: search and dispatch recheck it, while catalog changes reconcile registrations in the existing ToolBridge and continue through system reminders.

Clarify keeps decision authority with the user for material unknowns: the primary Agent asks until the goal is sufficiently specified, then completes it without a mandatory plan or approval step.

Plan is a human-governed contract with Drafting, AwaitingApproval, Executing, and Amending phases. Workspace edits are rejected before permissions until the submitted plan is approved. Approval freezes the plan and opens edits only for its execution; material deviation stops execution and requires an approved replacement. Workflow is unavailable throughout Plan. Completing or cancelling the contract returns the session to Normal (`default` on the ACP wire).

`plan_control` owns Plan transitions: `submit` and `amend` atomically persist a complete candidate before requesting approval, while `complete` and `cancel` terminate the contract. The artifact is control-plane state, never an editable workspace target and never an implied Agent file capability.

Workflow is the only public Workflow collaboration protocol. The tool is exposed only when both the turn-captured and live Behavior are Workflow; dispatch repeats that check, and Grow-originated writes to public Workflow directories are rejected outside it. Inside Workflow, Grow edits only session drafts; saved Project/User files change exclusively through validated atomic publish, while external-editor changes remain discoverable. A Workflow turn injects only focus/status/counts/diagnostics, then discovers by metadata before inspecting source. One clear candidate is announced and used; ambiguity is resolved by the user; argument-only variation reuses the Definition; orchestration changes derive a draft; no match creates a session draft.

The Workspace persists with the session and may contain several drafts, but has exactly one explicit Definition focus. “Current workflow” means that focus, never the latest Run. Publishing requires a validated current hash, an explicit Project or User scope, atomic replacement, and an optimistic baseline check. A successful publish removes the draft and focuses the saved Definition. A new draft's first successful Run offers Project/User save once per content hash.

Runs never own editable source. The same Definition may have several independently numbered and independently staged Runs. Editing, validation, or publishing changes only future Runs; pause/resume continues the original same-process snapshot. Only an `Active` public Run makes leaving Workflow a two-step confirmation; the Run continues in the background and can be managed again after re-entering Workflow. Paused and budget-limited Runs only warrant a reminder.

Deep Research is read-only evidence work with a mandatory terminal report. It uses a private Workflow Definition and runtime that never enters the public registry, command catalog, Workspace, or Runs panel and is not subject to the public Workflow gate. A terminal report is delivered for success, partial completion, verification failure, budget exhaustion, infrastructure failure, cancellation, and restart interruption. Natural completion delivers the report before returning to Normal; an interrupting Behavior switch delivers a cancellation report before applying the target Behavior.

Goal is objective-governed continuation. GoalTracker owns the objective and runtime status; Behavior owns only the collaboration protocol. The Agent may request verification, but only an independent verifier verdict of `Achieved` may complete the tracker and return to Normal. Missing verification, timeout, infrastructure failure, insufficient evidence, exhausted verification attempts, or exhausted budget pauses the Goal and keeps Goal Behavior selected.

The transition gateway rejects Plan, Goal, or Deep Research while an unrelated public Run is active. Leaving a non-terminal Plan, an active Goal, a running/paused Deep Research run, or Workflow with an active public Run requires a repeated selection of the same target within eight seconds. A paused or budget-limited public Run does not require confirmation. Pending confirmation is ephemeral and never persisted.
