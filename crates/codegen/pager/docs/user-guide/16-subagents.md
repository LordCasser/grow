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

`resume_from` creates a new child from a completed canonical lifecycle. It inherits the source transcript and model, while re-rendering the current Agent definition, system prompt, and tool runtime. Live grants and in-memory tool state are never copied.

The source must belong to the current parent session and use the same Agent type.

---

## Capability model

The Agent definition establishes a hard-eligible tool ceiling. The live capability mode grants a subset of that ceiling:

| Mode | Read | Write | Execute |
| --- | --- | --- | --- |
| `read-only` | Yes | No | No |
| `read-write` | Yes | Yes | No |
| `execute` | Yes | No | Yes |
| `all` | Yes | Yes | Yes |

Tools outside the current grant are not exposed to the model. A child can request one eligible boundary expansion with `request_tool_access`:

```json
{
  "target": { "type": "native", "capability": "execute" },
  "purpose": "Run the focused parser tests needed to validate the finding"
}
```

Native targets are `execute` and `read-write`. MCP access is requested per server:

```json
{
  "target": { "type": "mcp_server", "server": "github" },
  "purpose": "Inspect the issue referenced by the review"
}
```

The result is `granted`, `already_granted`, `denied`, or `unavailable`. An unavailable target lies outside the hard ceiling and cannot open an approval flow. Grants live only in the current child; they are not persisted, inherited, or restored by resuming.

Managed policy, hooks, and protected interactive boundaries remain authoritative after a grant.

---

## Permission mode

```toml
[subagents]
permission_mode = "auto"       # auto | ask | always-approve | follow
classifier_input = "context"   # context | request_only
```

- `auto` classifies only explicit capability-boundary requests. Normal calls already admitted by the live fence do not invoke the classifier.
- `ask` routes the request to the real child-session approval UI.
- `always-approve` still applies managed-policy clamps.
- `follow` reads the primary session's current decision mode for each request; it does not copy remembered grants.

Classifier input and verdicts are ephemeral and never become primary model history. Invalid output, timeout exhaustion, or a non-retryable provider failure denies only the requested expansion.

---

## MCP inheritance

Children inherit the parent's connected and enabled MCP catalog by default. Inheritance establishes eligibility, not authorization. `search_tool` reports whether a result is granted, and `use_tool` performs the concrete call only after the required server grant and permission decision.

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

`[subagents].max_depth` limits recursive spawning. At the boundary the spawn tool is removed from the child's eligible toolset. A live capability grant cannot restore it or bypass Goal/Workflow ownership.

Use subagents for independent investigation, implementation, focused testing, and review. Keep tightly interactive or trivially small work in the parent session.
