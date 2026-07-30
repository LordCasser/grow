# Prompt, Agent, Behavior, and Tool Boundaries

Grow composes an Agent session from independent layers. The layers have a fixed order and later layers may only narrow capabilities established earlier.

```text
Mandatory Core
  + Audience (primary | subagent)
  + Agent Role (Markdown body)
  + Active Behavior (none | clarify | plan)
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

Behavior is session state, not part of `AgentDefinition`. ACP maps `default` to no Behavior, `ask` to Clarify, and `plan` to Plan. A Task call may select the same Behavior independently of `subagent_type`. A Behavior never adds a tool, changes which Agent may be delegated to, changes capability mode, or bypasses permission checks.

The effective call rule is:

```text
registered by Tool Preset / Registry
  ∩ Agent tool and subagent policy
  ∩ subagent capability and depth policy
  ∩ active Behavior gate
  ∩ session permission decision
```

Plan preserves the existing Pending, Active, ExitPending, approval, reentry, compaction, and restore lifecycle in `BehaviorController`. While Plan is active, every ordinary `AccessKind::Edit` call is rejected before the permission manager. Other already-authorized calls continue into the ordinary permission flow and are guided to investigation and verification only.

The completed plan is passed as `exit_plan_mode(plan=...)`. The host validates it, atomically writes it to the session-owned artifact, and only then requests approval. The artifact is control-plane state, never an editable workspace target and never an implied Agent file capability.

Dynamic Workflow is intentionally outside the current Behavior set. A later workflow design should attach to the same controller and one-way capability rule rather than introducing role/tool combination Agents.
