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

Audience describes only ownership: the primary Agent owns the user-facing result, while a subagent owns only its delegated task. Audience never declares a role, toolset, or Behavior.

Built-in and user-defined Agents are Markdown files with YAML frontmatter. The Markdown body is the role and use-case prompt. Tool configuration is explicit and independent:

```yaml
promptComposition: extend
toolPreset: grow-build
additionalTools: []
disallowedTools: []
subagents:
  allow: []
  deny: []
```

`toolPreset` is resolved first, followed by `additionalTools`, fixed runtime injection, availability/depth/capability filtering, Agent denies and subagent policy, session clamps, and ToolBridge finalization. `ToolKind`, `ToolMetadata`, registry finalization, template name resolution, and `ToolServerConfig.behavior_preset` retain their existing responsibilities.

Behavior is mutually-exclusive primary-session state, not part of `AgentDefinition`. The state is exactly `Normal | Clarify | Plan(phase) | Workflow | DeepResearch { run_id } | Goal`. ACP maps these to `default`, `ask`, `plan`, `workflow`, `deep_research`, and `goal`. Every mode request, including prompt metadata and slash-command shortcuts, passes through the same Behavior transition gateway. Delegated Agents receive an explicit role and task; they never inherit or select a user-facing Behavior. A Behavior never adds a tool, changes which Agent may be delegated to, changes capability mode, or bypasses permission checks.

The layers have different owners and must not be substituted for one another:

- **Behavior** is the primary Agent's protocol for advancing the user's request.
- **Role** is a delegated Agent's bounded responsibility.
- **WorkflowRun** is an execution/journal instance owned by the workflow engine. Leaving Dynamic Workflow does not implicitly cancel an ordinary run.
- **GoalTracker** is the persistent objective, continuation, and verification instance used while Goal Behavior is selected.

The effective call rule is:

```text
registered by Tool Preset / Registry
  ∩ Agent tool and subagent policy
  ∩ subagent capability and depth policy
  ∩ active Behavior gate
  ∩ session permission decision
```

Clarify keeps decision authority with the user for material unknowns: the primary Agent asks until the goal is sufficiently specified, then completes it without a mandatory plan or approval step.

Plan is a human-governed contract with Drafting, AwaitingApproval, Executing, and Amending phases. Workspace edits are rejected before permissions until the submitted plan is approved. Approval freezes the plan and opens edits only for its execution; material deviation stops execution and requires an approved replacement. Dynamic Workflow is unavailable throughout Plan. Completing or cancelling the contract returns the session to Normal (`default` on the ACP wire).

`plan_control` owns Plan transitions: `submit` and `amend` atomically persist a complete candidate before requesting approval, while `complete` and `cancel` terminate the contract. The artifact is control-plane state, never an editable workspace target and never an implied Agent file capability.

Dynamic Workflow is agent-governed dynamic sub-planning. The primary Agent scouts a bounded phase, launches at most one suitable workflow run for that phase, yields instead of polling, and decides the next phase from the completion result. WorkflowRun remains an independent runtime/journal entity and never changes Behavior. Dynamic Workflow is offered only when the finalized ToolBridge contains the Workflow tool; each run has independent cumulative (`agent_budget`) and simultaneous (`max_concurrency`) child limits.

Deep Research is read-only evidence work with a mandatory terminal report. It uses a private Workflow definition and runtime, but the report contract and Behavior-owned run lifecycle are not public Workflow semantics. A terminal report is delivered for success, partial completion, verification failure, budget exhaustion, infrastructure failure, cancellation, and restart interruption. Natural completion delivers the report before returning to Normal; an interrupting Behavior switch delivers a cancellation report before applying the target Behavior.

Goal is objective-governed continuation. GoalTracker owns the objective and runtime status; Behavior owns only the collaboration protocol. The Agent may request verification, but only an independent verifier verdict of `Achieved` may complete the tracker and return to Normal. Missing verification, timeout, infrastructure failure, insufficient evidence, exhausted verification attempts, or exhausted budget pauses the Goal and keeps Goal Behavior selected.

The transition gateway rejects Plan, Goal, or Deep Research while an unrelated WorkflowRun is non-terminal. Leaving a non-terminal Plan, an active Goal, or a running/paused Deep Research run requires a repeated selection of the same target within three seconds. Pending confirmation is ephemeral and never persisted.
