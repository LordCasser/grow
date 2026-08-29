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

1. commits the spawn fact to the parent Timeline;
2. atomically creates the child session with a seed-source fact;
3. runs the child against its own Timeline and runtime;
4. commits the child's terminal result;
5. closes the parent spawn by referencing that exact child event.

Successful non-empty output is stored once as an immutable content-addressed artifact. There is no parallel metadata or output-file lifecycle.

After restart, Grow reconstructs child rows and terminal state from the parent and child Timeline facts. A broken link, missing child, or mismatched terminal metrics fails closed instead of selecting another transcript heuristically.

---

## Built-in Agent types

The `subagent_type` argument selects the child definition:

| Type | Purpose |
| --- | --- |
| `general-purpose` | General implementation and investigation work. |
| `explore` | Read-focused codebase investigation. It can request execution when the definition allows it, but cannot request edits. |
| `plan` | Exploration and implementation planning without file edits. |

Project and user definitions can add new types or intentionally shadow a built-in name.

---

## Spawning a child

`spawn_subagent` accepts:

| Parameter | Meaning |
| --- | --- |
| `prompt` | Complete delegated task. |
| `description` | Short task label. |
| `subagent_type` | Agent definition; defaults to `general-purpose`. |
| `background` | Return immediately with a child ID. |
| `capability_mode` | Initial grant: `read-only`, `read-write`, `execute`, or `all`. |
| `isolation` | `none` or an isolated git `worktree`. |
| `resume_from` | Continue a completed child by ID. |
| `cwd` | Child working directory; mutually exclusive with worktree isolation. |

Retrieve a background result with `get_command_or_subagent_output`.

### Resume semantics

`resume_from` creates a new child from a completed canonical lifecycle. It inherits the source transcript and model, while re-rendering the current Agent definition, system prompt, and tool runtime. In-memory permits and transport incarnations are never copied.

The source must belong to the current parent session and use the same Agent type.

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
