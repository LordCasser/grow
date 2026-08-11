# Prompt, Agent, Behavior, and Tool Boundaries

Grow composes an Agent session from independent layers. The layers have a fixed order and later layers may only narrow capabilities established earlier.

```text
Mandatory Core
  + Audience (primary | subagent)
  + Agent Role (Markdown body)
  + Active Behavior (normal | clarify | plan | workflow | deep-research | goal)
  + Runtime Context
```

Mandatory Core contains instruction priority, action safety, tool-use rules, project-instruction scoping, output rules, and Grow client context. It is always rendered. `promptComposition: full` replaces the standard role foundation; it does not replace Mandatory Core, Audience, an active Behavior, or Runtime Context.

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

`toolPreset` is resolved first, followed by `additionalTools`, fixed runtime injection, Agent denies and subagent policy, session clamps, depth/ownership/plugin/MCP eligibility, and ToolBridge finalization. Requestable native Execute/ReadWrite eligibility requires both an authored matching kind from `toolPreset`/`additionalTools` and a surviving implementation in the finalized bridge; runtime injection cannot silently expand that ceiling. For a subagent, `capabilityMode` no longer deletes eligible tools: it seeds the child-local current grant set. Model tool definitions and call dispatch both filter the finalized bridge through that grant set. `ToolKind`, `ToolMetadata`, registry finalization, template name resolution, and `ToolServerConfig.behavior_preset` retain their existing responsibilities.

The built-in Agent, tool, and session surface is Grow-native. External vendor schemas are not exposed as Agent profiles, tool presets, namespaces, ignored frontmatter fields, or session scanners. Agent files use the documented Grow schema and reject unknown keys; source provenance does not create a vendor execution mode.

Behavior is mutually-exclusive primary-session state, not part of `AgentDefinition`. The state is exactly `Normal | Clarify | Plan(phase) | Workflow | DeepResearch { run_id } | Goal`. ACP maps these to `default`, `ask`, `plan`, `workflow`, `deep_research`, and `goal`. Every mode request, including prompt metadata and slash-command shortcuts, passes through the same Behavior transition gateway. Delegated Agents receive an explicit role and task; they never inherit or select a user-facing Behavior. A Behavior never adds a tool, changes which Agent may be delegated to, changes capability mode, or bypasses permission checks.

The layers have different owners and must not be substituted for one another:

- **Behavior** is the primary Agent's protocol for advancing the user's request.
- **Role** is a delegated Agent's bounded responsibility.
- **WorkflowRun** is an execution/journal instance owned by the workflow engine. Leaving Static Workflow does not implicitly cancel an ordinary run.
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

Plan is a human-governed contract with Drafting, AwaitingApproval, Executing, and Amending phases. Workspace edits are rejected before permissions until the submitted plan is approved. Approval freezes the plan and opens edits only for its execution; material deviation stops execution and requires an approved replacement. Static Workflow is unavailable throughout Plan. Completing or cancelling the contract returns the session to Normal (`default` on the ACP wire).

`plan_control` owns Plan transitions: `submit` and `amend` atomically persist a complete candidate before requesting approval, while `complete` and `cancel` terminate the contract. The artifact is control-plane state, never an editable workspace target and never an implied Agent file capability.

Static Workflow is agent-governed per-phase scripted sub-planning. The primary Agent personally scouts the phase's central evidence, translates that understanding into independent bounded jobs, launches at most one suitable workflow run for that phase, yields instead of polling, and decides the next phase from the completion result. A workflow must not merely wrap the user's whole request in one coarse child task. WorkflowRun remains an independent runtime/journal entity and never changes Behavior. Static Workflow is offered only when the finalized ToolBridge contains the Workflow tool; each run has independent cumulative (`agent_budget`) and simultaneous (`max_concurrency`) child limits.

Deep Research is read-only evidence work with a mandatory terminal report. It uses a private Workflow definition and runtime, but the report contract and Behavior-owned run lifecycle are not public Workflow semantics. A terminal report is delivered for success, partial completion, verification failure, budget exhaustion, infrastructure failure, cancellation, and restart interruption. Natural completion delivers the report before returning to Normal; an interrupting Behavior switch delivers a cancellation report before applying the target Behavior.

Goal is objective-governed continuation. GoalTracker owns the objective and runtime status; Behavior owns only the collaboration protocol. The Agent may request verification, but only an independent verifier verdict of `Achieved` may complete the tracker and return to Normal. Missing verification, timeout, infrastructure failure, insufficient evidence, exhausted verification attempts, or exhausted budget pauses the Goal and keeps Goal Behavior selected.

The transition gateway rejects Plan, Goal, or Deep Research while an unrelated WorkflowRun is non-terminal. Leaving a non-terminal Plan, an active Goal, or a running/paused Deep Research run requires a repeated selection of the same target within eight seconds (surfaced in the TUI as an Enter-to-confirm / Esc-to-cancel warning). Pending confirmation is ephemeral and never persisted.
