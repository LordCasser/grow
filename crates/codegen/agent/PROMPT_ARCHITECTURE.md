# Prompt, Agent, Behavior, and Tool Boundaries

Grow composes an Agent session from independent layers. The layers have a fixed order and later layers may only narrow capabilities established earlier.

```text
Stable System Head
  + Mandatory Core
  + Audience (primary | subagent)

Timeline Surface
  + Control-event-anchored Agent policy/role transition items in the live tail
  + Control-event-anchored Active Behavior transition items in the live tail
  + Runtime, project, skill, capability, and session-rule context items
  + Retrieved memory evidence in the live tail
```

Mandatory Core contains instruction priority, action safety, generic tool-use rules, project-instruction scoping, output rules, and Grow client context. Together with the fixed Audience it forms the one stable system head. The head contains no Agent tool names, role prose, retrieved memory, client rules, model-specific concise variant, or Behavior. It is seeded once; model changes, Agent switches, client attach, memory retrieval, and compaction never replace it. `promptComposition: full` only removes optional standard guidance from the typed Agent layer and uses the Agent Markdown body as that layer's authored role; it cannot replace the stable head.

Runtime facts do not belong to the system prompt. At session start, the shell renders one typed `RuntimeContextSnapshot` as a user-role message containing the visible workspace, OS, shell, local date, and optional VCS snapshot, then durably appends it to Timeline. Agent definitions cannot override its shape. Skills, project instructions, MCP catalogs, capability changes, client-supplied session rules, and reminders each publish their own typed Timeline-backed messages instead of being copied into either the system prompt or the runtime snapshot. Retrieved memory has one path: an append-only `MemoryContext` item at the live tail, durably committed before the request is assembled and idempotent for identical evidence.

Every ordinary model request carries one provider-routing cache key derived from Timeline identity, the latest rewind branch anchor, and the concrete backend/base URL/model route. Appending messages, changing Behavior, or selecting another Agent does not create a new lineage key; fork, rewind, and model-route changes do. The key is routing metadata, not a content fingerprint: provider-visible content still determines whether a prefix actually matches.

Audience defines ownership boundaries: the primary Agent owns the user-facing result, the task-wide understanding needed to produce it, and the integration of delegated results, while a subagent owns only its delegated task. Primary ownership cannot be transferred wholesale: delegation may extend coverage or isolate bounded work, but the primary Agent directly examines central evidence, retains cross-cutting synthesis, and continues independent work while children run whenever useful work remains. Audience never declares a role, toolset, or Behavior.

Built-in and user-defined Agents are Markdown files with YAML frontmatter. The Markdown body is the role and use-case prompt. The rendered Agent layer contains optional standard guidance, the authored role, and tool-dependent session extensions; it is never copied into the system head. The initial selection and every later switch append one typed `system.role` Control context whose snapshot carries the same Agent identity. A role-less `full` Agent with no extension emits an explicit reset, so selecting it retires earlier Agent instructions rather than leaving them active by omission. Tool configuration is explicit and independent:

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

`toolPreset` is resolved first, followed by `additionalTools`, fixed runtime injection, Agent denies and subagent policy, session clamps, depth/ownership/plugin/MCP eligibility, and ToolBridge finalization. Native Execute/ReadWrite hard eligibility requires both an authored exact identity from `toolPreset`/`additionalTools` and a surviving implementation in the finalized bridge; runtime injection cannot silently expand that ceiling. For a subagent, `capabilityMode` establishes immutable initial RWX rather than deleting latent tools or creating mutable session authority. A projected call inside initial RWX follows the ordinary permission path; a hard-eligible call outside it enters Ask/Auto for one exact frozen call. `ToolKind` remains presentation/discovery metadata; the finalized descriptor, projected RWX, exact identity, and call-bound permit own authorization.

The built-in Agent, tool, and session surface is Grow-native. External vendor schemas are not exposed as Agent profiles, tool presets, namespaces, ignored frontmatter fields, or session scanners. Agent files use the documented Grow schema and reject unknown keys; source provenance does not create a vendor execution mode.

Behavior is mutually-exclusive primary-session state, not part of `AgentDefinition`. The state is exactly `Normal | Clarify | Plan(phase) | Workflow | Goal`. ACP maps these to `normal`, `ask`, `plan`, `workflow`, and `goal`. Every external mode request passes through the same gateway and shares the foreground-admission mutex, so it commits only while idle. Model, reasoning-effort, and Agent selections instead enter one ordered next-turn control queue: they are accepted in every Behavior while a foreground is active, never mutate that turn's captured route, and commit under an exclusive idle fence before any queued prompt, notification, compaction, or Goal continuation. Catalog validate/freeze and actor enqueue share one publication transaction; load waits and actor acknowledgements stay outside that lock. The Pager serializes control RPCs per exact root/child session and binds completions to a reconnect generation, so rapid selections cannot reorder or release a prompt early. Agent selection then builds the candidate harness, durably appends its typed role context and Agent identity, swaps and fully binds the live harness, then atomically updates the shared Agent profile used by nested delegation and publishes the client projection; a failed append leaves the prior Agent intact. A selection and its synthetic user protocol item are one durable Timeline Control event. Idle contexts enter Surface immediately; turn-internal transitions such as Goal completion remain pending until that turn's durable terminal, with the latest pending transition in each typed layer activated in causal event order. Thus later output stays after the protocol that conditioned it, tool call/result adjacency remains valid, and subsequent requests preserve the provider-visible prefix. Later switches append a new transition; switching to `Normal` appends an explicit reset that retires earlier special protocols. Neither Agent nor Behavior transitions mutate the system head or rebuild message history. If compaction shadows the effective context of either typed layer, the compaction path immediately appends the exact effective item again before sampling can resume; a transition still pending at that boundary does not displace the context that currently governs the turn. Reload and turn admission repeat this deterministic repair for crash gaps. Older shadowed contexts never reactivate. Delegated Agents receive an explicit role and task; they never inherit or select a user-facing Behavior. A Behavior never adds a tool, changes which Agent may be delegated to, changes capability mode, or bypasses permission checks.

The layers have different owners and must not be substituted for one another:

- **Behavior** is the primary Agent's protocol for advancing the user's request.
- **Role** is a delegated Agent's bounded responsibility.
- **Workflow Workspace** is session-owned control state: session drafts, one explicit Definition focus, origin/baseline/current/validated hashes, publish conflicts, and per-hash save reminders.
- **Workflow Definition** is editable content in exactly one scope (`Session | Project | User`). Saved Definitions are derived into session drafts before Grow modifies them.
- **Workflow Run** is an immutable execution/journal snapshot owned by the workflow engine. It persists the Definition id, scope, content hash, script, and args. Leaving Workflow does not cancel an ordinary Run.
- **GoalTracker** is the sole durable Goal record: objective definition, lifecycle status, token budget, settled usage, elapsed time, and status message. SessionActor owns idle continuation; there is no persisted planner, verifier, task graph, or second Goal lifecycle.

The effective call rule is:

```text
registered by Tool Preset / Registry
  ∩ Agent tool and subagent policy
  ∩ session clamp, depth, ownership, plugin trust, and MCP inheritance
  ∩ active Behavior gate
  ∩ projected call RWX
  → inside initial RWX: ordinary permission path
  → outside initial RWX but hard-eligible: Ask/Auto exact-call decision
  → call-bound permit + dispatch-time identity/argument/transport revalidation
```

The first three lines form the subagent's immutable hard eligibility ceiling. The capability catalog truthfully distinguishes identities whose entire descriptor ceiling is covered, identities whose exact arguments still require call projection, and forbidden identities; it is not an authorization API. A projected locked call goes directly through the ordinary permission manager. Allowing one does not mutate child authority, expose a server for the rest of the session, or change the next sample. The permit binds call id, exact tool/dispatch target, canonical arguments, cwd, projected RWX, actor epoch, and MCP transport generation; dispatch consumes it once and repeats hard-eligibility and transport checks.

The capability catalog is an audience/runtime layer, not role prose. The subagent audience explains exact-call authority; a native system reminder lists fully covered, call-projected, and forbidden identities; MCP reminders list inherited eligible servers. `search_tool` searches only the finalized child MCP index and labels results `call_bound`. The parent's model-visible MCP allowset is a live, depth-preserving authority: search and dispatch recheck it, while catalog changes reconcile registrations in the existing ToolBridge and continue through system reminders.

Clarify keeps decision authority with the user for material unknowns: the primary Agent asks until the goal is sufficiently specified, then completes it without a mandatory plan or approval step.

Plan is a human-governed contract with Drafting, AwaitingApproval, Executing, and Amending phases. Workspace edits are rejected before permissions until the submitted plan is approved. Approval freezes the plan and opens edits only for its execution; material deviation stops execution and requires an approved replacement. Workflow is unavailable throughout Plan. Completing or cancelling the contract returns the session to Normal (`normal` on the ACP wire).

`plan_control` owns Plan transitions: `submit` and `amend` atomically persist a complete candidate before requesting approval, while `complete` and `cancel` terminate the contract. The artifact is control-plane state, never an editable workspace target and never an implied Agent file capability.

Workflow is the only public Workflow collaboration protocol. The tool is exposed only when both the turn-captured and live Behavior are Workflow; dispatch repeats that check, and Grow-originated writes to public Workflow directories are rejected outside it. Inside Workflow, Grow edits only session drafts; saved Project/User files change exclusively through validated atomic publish, while external-editor changes remain discoverable. A Workflow turn injects only focus/status/counts/diagnostics, then discovers by metadata before inspecting source. One clear candidate is announced and used; ambiguity is resolved by the user; argument-only variation reuses the Definition; orchestration changes derive a draft; no match creates a session draft.

The Workspace persists with the session and may contain several drafts, but has exactly one explicit Definition focus. “Current workflow” means that focus, never the latest Run. Publishing requires a validated current hash, an explicit Project or User scope, atomic replacement, and an optimistic baseline check. A successful publish removes the draft and focuses the saved Definition. A new draft's first successful Run offers Project/User save once per content hash.

Runs never own editable source. The same Definition may have several independently numbered and independently staged Runs. Editing, validation, or publishing changes only future Runs; pause/resume continues the original same-process snapshot. Only an `Active` public Run makes leaving Workflow a two-step confirmation; the Run continues in the background and can be managed again after re-entering Workflow. Paused and budget-limited Runs only warrant a reminder.

Deep Research is version-managed by the builtin extractor into `~/.grow/workflows/deep-research.rhai`, then discovered as an ordinary User workflow. It is launched, persisted, resumed, displayed, and managed through the same Registry, Workflow Workspace, and Run lifecycle as every other Definition. It has no Builtin scope, Behavior, private registry, private Run projection, special foreground permission gate, or host-owned terminal-report path; its research and report contract lives entirely in the Rhai Definition.

Goal is objective-governed continuation. GoalTracker owns only the durable objective and accounting/lifecycle record; Behavior owns the active collaboration protocol, while SessionActor arbitrates each continuation behind foreground work, the user FIFO, and durable notifications. Every continuation receives a fresh directive to audit the entire objective against concrete evidence before choosing the next small slice. Remaining work uses ordinary short-lived `todo_write` and `task` execution context, never a persisted planner, verifier, board, or second Goal state. Only Active Goal selects Goal Behavior. Complete, paused, blocked, and budget-limited states stop continuation and release to Normal; the durable Goal remains available for edit, restart, budget change, or clear.

The transition gateway rejects Plan or Goal while an unrelated Workflow Run is active. Leaving a non-terminal Plan or Workflow with an active Run requires a repeated selection of the same target within eight seconds. Active Goal must be stopped before another Behavior is selected; stopped Goal state is orthogonal and does not block Behavior changes. A paused or budget-limited Run does not require confirmation. Pending confirmation is ephemeral and never persisted.
