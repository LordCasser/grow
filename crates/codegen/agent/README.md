# `agent`

Agent definition parsing, typed role projection, and session-scoped tool assembly.

An `Agent` binds an `AgentDefinition` to its rendered role context and the
current session's tool bridge. It is session-bound, not a portable runtime
policy container. The session owns model/provider selection, permissions,
reminder scheduling, compaction policy, and their lifecycle state; switching
Agent changes only the role context and authored tool policy.

## Quick Start

### From a definition file

Agent definitions are **Markdown files with YAML frontmatter**. Project
definitions live below `.grow/agents/`. User definitions
load from `~/.grow/agents/`.

```rust
use agent::{AgentDefinition, AgentBuilder};
use std::sync::Arc;
use tools::computer::local::{LocalTerminalBackend, SearchShadowConfig};
use tools::notification::ToolNotificationHandle;

// 1. Parse the definition file
let def = AgentDefinition::from_file(".grow/agents/code-reviewer.md")?;

// 2. Build the agent
let terminal = Arc::new(LocalTerminalBackend::new_local(SearchShadowConfig::default()));
let agent = AgentBuilder::new(cwd, terminal, ToolNotificationHandle::noop())
    .from_definition(def)
    .build()
    .await?;

// 3. Use it
println!("Agent: {}", agent.name());
println!("Prompt: {}", agent.system_prompt());
let tool_defs = agent.tool_definitions().await;
```

### Programmatic (no file)

```rust
let terminal = Arc::new(LocalTerminalBackend::new_local(SearchShadowConfig::default()));
let agent = AgentBuilder::new(cwd, terminal, ToolNotificationHandle::noop())
    .with_name("my-agent")
    .with_description("A custom agent")
    .with_tools(vec!["read_file".into(), "grep".into()])
    .build()
    .await?;
```

### Discover all definitions

```rust
use agent::discovery;

// Find all recursively discovered Agent Markdown files
let definitions = discovery::discover(&cwd);

// Find a specific agent by path-derived ID
let reviewer = discovery::by_name("code-reviewer");

// Find with project-level priority
let agent = discovery::by_name_in_cwd("my-agent", &cwd);
```

## Agent Definition File Format

Agent definitions are Markdown files with YAML frontmatter:

```markdown
---
description: What this agent does
# ... additional config fields
---

Agent role body goes here...
```

The **frontmatter** (between `---` delimiters) is YAML configuration.
The **body** (after the closing `---`) is Agent-scoped role content. It is
rendered into the typed `system.role` Timeline layer, never into the stable
system head.
The Agent ID comes from its relative path, not `name`: for example,
`review/backend.md` is selected as `review/backend`. A frontmatter `name`
is accepted for interoperability but does not override that stable ID.

### Minimal example (extends base template)

```markdown
---
description: Reviews code for quality and security
tools:
  - read_file
  - grep
  - list_dir
---

You are a senior code reviewer. Analyze code and provide
actionable feedback organized by severity.
```

With `promptComposition: extend` (the default), the typed Agent layer contains
standard guidance followed by the body and any tool-dependent session
extension. The stable mandatory foundation and audience remain in the system
head; active Behavior is a separate Control layer.

### Full role composition

```markdown
---
name: custom-agent
description: Agent whose role omits standard guidance
promptComposition: full
tools:
  - read_file
  - search_replace
  - run_terminal_cmd
---

You are a custom agent.

Use ${{ tools.read_file }} to read files.
Use ${{ tools.search_replace }} to edit files.

${%- if tools.run_terminal_cmd %}
Use ${{ tools.run_terminal_cmd }} for shell commands.
${%- endif %}
```

With `promptComposition: full`, the typed Agent layer omits optional standard
guidance and renders the body through MiniJinja. Mandatory foundation and
audience remain in the stable system head; Behavior remains a separate Control
layer; tool-dependent session extensions remain in the Agent layer.

### With a completion requirement

```markdown
---
name: completion-worker
description: Worker agent that must signal completion before ending a turn
subagentOnly: true
completionRequirement:
  tool: complete_task
  reminder: >
    You stopped without calling `complete_task`.
    Please continue and call it when done.
  recovery:
    maxRetries: 5
    baseDelayMs: 5000
    maxDelayMs: 60000
---

You are a worker agent in a multi-agent workflow.
You MUST call `complete_task` before ending your response.
```

## Frontmatter Schema Reference

The repository-root [`agent.md.example`](../../../agent.md.example) is the
authoritative, copy-ready schema example. It exercises every supported field
in a parser regression test, including prompt assembly, exact tool
configuration, Skill and AGENTS.md discovery, subagent defaults, MCP
inheritance and owned servers, hooks, memory, completion requirements, and the
prompt-composition boundary.

Top-level frontmatter keys use **camelCase**. The records nested below
`additionalTools` use the `tools` wire names in **snake_case**. The
`toolPreset` is resolved first, `additionalTools` are layered next, and `tools`
plus `disallowedTools` later filter the assembled result. `subagents.allow`
and `subagents.deny` independently control delegation targets.

### Grow schema boundary

Grow accepts only the documented camelCase Agent frontmatter. Unknown fields,
including vendor-style `mode`, `permission`, `permissions`, `model`, `variant`,
and `request`, fail parsing. An Agent never selects a provider/model, changes
the session permission mode, or declares a parent/child Agent relationship.

All enabled Agent definitions are peers. Switching Agent changes the prompt
profile and tool assembly only; switching model changes the model only. A new
session uses the global default Agent and model. A resumed session restores the
last Agent and model persisted for that session independently.

## Prompt Assembly

Grow's built-in system prompts live as Markdown in `prompts/`. They are the
single source of truth and are embedded into the binary at compile time; no
prompt files or generation step are required at runtime.

```
Stable system head
1. Mandatory Core
2. Audience

Typed Agent Control layer
extend: Standard guidance + Markdown role body + tool-dependent extensions
full:   Markdown role body + tool-dependent extensions

Typed Behavior Control layer
Active Behavior protocol
```

The system prompt does not carry mutable runtime facts. The shell appends one
typed runtime snapshot as a durable user-role Timeline message at session
start. Its visible workspace, OS, shell, local date, and optional VCS status
have one canonical renderer and cannot be replaced by Agent frontmatter.
Skills, AGENTS.md instructions, and MCP catalogs are separate Timeline-backed
messages with their own owners.

### Template Variables (full mode)

| Variable | Description |
|---|---|
| `${{ tools.read_file }}` | Resolved name for `read_file` (or empty if disabled) |
| `${{ tools.search_replace }}` | Resolved name for `search_replace` |
| `${{ tools.run_terminal_cmd }}` | Resolved name for `run_terminal_cmd` |
| `${{ tools.grep }}` | Resolved name for `grep` |
| `${{ tools.list_dir }}` | Resolved name for `list_dir` |
| `${{ tools.todo_write }}` | Resolved name for `todo_write` |
| `${{ tools.skill }}` | Resolved name for `skill` |
| `${{ tools.get_task_output }}` | Resolved name for `get_task_output` |
| `${{ tools.kill_task }}` | Resolved name for `kill_task` |

Conditionals: `${%- if tools.todo_write %}...${%- endif %}` — block
is omitted when the tool is disabled.

## Discovery Rules

Agent definitions are discovered from multiple locations with priority:

1. **Project-level** (highest priority): `.grow/agents/**/*.md`, walking
   from `cwd` up to the git repository root. Files found closer to
   `cwd` take priority.
2. **User-level**: `~/.grow/agents/**/*.md`
3. **Built-in and injected definitions**

Name-based dedup ensures the highest-priority definition wins. For
example, a project `.grow/agents/code-reviewer.md` shadows a
user-level definition with the same ID.

## Crate Relationships

```
┌──────────────────┐
│  agent  │  ← This crate
│  (Agent, Builder, │
│   Definition)     │
└────────┬─────────┘
         │ depends on
         ▼
┌──────────────────┐
│  tools  │
│  (ToolBridge,    │
│   ToolRegistry,  │
│   ToolState)     │
└────────▲─────────┘
         │ depends on
┌────────┴─────────┐
│  shell  │  uses AgentBuilder to create
│  (session host)  │  Agent during session setup
└──────────────────┘
```

- **`tools`**: Provides `ToolBridge`, `ToolRegistry`,
  `ToolState`, `SystemReminderLayer`, and tool implementations.
  `agent` depends on it for tool setup.
- **`shell`**: The application shell. Uses `AgentBuilder`
  to construct an `Agent` during session creation. The shell
  re-exports some modules from `agent` (AGENTS.md
  discovery, skills discovery, base prompt rendering).

## Built-in Agents

| Name | Prompt Mode | Description |
|---|---|---|
| `grow-build` | extend | Default agent for software engineering tasks |
| `browser-use` | full | Web browsing and interaction agent |

## Error Handling

`AgentBuilder::build()` returns `Result<Agent, AgentBuildError>`:

| Error | When |
|---|---|
| `ParseError` | Bad YAML, missing `---`, wrong types |
| `MissingField` | Required field (`name`/`description`) absent |
| `UnknownToolOverride` | `toolNameOverrides` references nonexistent tool |
| `IoError` | File read error during AGENTS.md/skills discovery |
| `MiniJinjaError` | Template rendering failure |

Unknown frontmatter fields are rejected so misspellings or foreign Agent
schemas cannot silently change Agent behavior.

## Development

```bash
# Check
cargo check -p agent

# Test
cargo test -p agent

# Clippy
cargo clippy -p agent --fix --allow-dirty

# Format
cargo fmt --all
```
