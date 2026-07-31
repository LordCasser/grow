# `grow-agent`

Agent builder, definition parsing, and system prompt assembly.

This crate extracts a first-class `Agent` type from `grow-shell`.
An `Agent` bundles a prompt profile, tool policy, system-reminder policy,
and compaction policy into a portable object that any host can consume.
Model and permission selection are session concerns and are deliberately
independent from Agent selection — whether that host is
`grow-shell`, another in-process host, or a headless batch runner.

## Quick Start

### From a definition file

Agent definitions are **Markdown files with YAML frontmatter**. Project
definitions live below `.grow/agents/` or `.claude/agents/`. User definitions
load from `~/.grow/agents/`.

```rust
use grow_agent::{AgentDefinition, AgentBuilder};
use grow_tools::notification::ToolNotificationHandle;

// 1. Parse the definition file
let def = AgentDefinition::from_file(".grow/agents/code-reviewer.md")?;

// 2. Build the agent
let agent = AgentBuilder::new(cwd, None, ToolNotificationHandle::noop())
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
let agent = AgentBuilder::new(cwd, None, ToolNotificationHandle::noop())
    .with_name("my-agent")
    .with_description("A custom agent")
    .with_tools(vec!["read_file".into(), "grep".into()])
    .build()
    .await?;
```

### Discover all definitions

```rust
use grow_agent::discovery;

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

System prompt body goes here...
```

The **frontmatter** (between `---` delimiters) is YAML configuration.
The **body** (after the closing `---`) is the system prompt content.
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

With `promptComposition: extend` (the default), the body is appended after
the mandatory foundation, audience, and standard guidance. Runtime context is
added last. The author only writes role-specific content.

### Full prompt override

```markdown
---
name: custom-agent
description: Agent with full control over the system prompt
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

<user_info>
OS: ${{ os_name }}
Shell: ${{ shell_path }}
Working Directory: ${{ working_directory }}
Date: ${{ current_date }}
</user_info>
```

With `promptComposition: full`, the body replaces the optional standard/role
guidance and is rendered through MiniJinja. Mandatory foundation, audience,
and runtime context remain in force.

### With completion requirement (orchestrated mode)

```markdown
---
name: orchestrator-worker
description: Worker agent that must signal completion before ending a turn
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

You are a worker agent in an orchestrated multi-agent workflow.
You MUST call `complete_task` before ending your response.
```

## Frontmatter Schema Reference

The repository-root [`agent.md.example`](../../../agent.md.example) is the
authoritative, copy-ready schema example. It exercises every supported field
in a parser regression test, including prompt assembly, exact tool
configuration, Skill and AGENTS.md discovery, subagent defaults, MCP
inheritance and owned servers, hooks, memory, completion requirements, and the
first-user-message template.

Top-level frontmatter keys use **camelCase**. The records nested below
`additionalTools` use the `grow-tools` wire names in **snake_case**. The
`toolPreset` is resolved first, `additionalTools` are layered next, and `tools`
plus `disallowedTools` later filter the assembled result. `subagents.allow`
and `subagents.deny` independently control delegation targets.

### Harness and OpenCode compatibility

Grow accepts the common Markdown shape used by Harness and OpenCode. The
foreign frontmatter keys `mode`, `permission`, `permissions`, `model`,
`variant`, and `request` are intentionally ignored: an Agent never selects a
provider/model, changes the permission mode, or declares a parent/child Agent
relationship. Unknown keys still fail parsing so misspellings do not silently
change behavior. `permissionMode` and model fields from older Grow Agent files
are likewise not applied to session state.

All enabled Agent definitions are peers. Switching Agent changes the prompt
profile and tool assembly only; switching model changes the model only. A new
session uses the global default Agent and model. A resumed session restores the
last Agent and model persisted for that session independently.

## Prompt Assembly

Grow's built-in system prompts live as Markdown in `prompts/`. They are the
single source of truth and are embedded into the binary at compile time; no
prompt files or generation step are required at runtime.

```
promptComposition: extend              promptComposition: full
─────────────────────────              ────────────────────────
1. Mandatory Core                      1. Mandatory Core
2. Audience                            2. Audience
3. Standard guidance                   3. Markdown role body
4. Markdown role body                  4. Runtime Context
5. Runtime Context
```

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
| `${{ os_name }}` | Operating system (e.g. `"macos"`, `"linux"`) |
| `${{ shell_path }}` | Shell path (e.g. `"/bin/zsh"`) |
| `${{ working_directory }}` | Workspace path |
| `${{ current_date }}` | Current date in the user's local timezone (`YYYY-MM-DD`) |

Conditionals: `${%- if tools.todo_write %}...${%- endif %}` — block
is omitted when the tool is disabled.

## Discovery Rules

Agent definitions are discovered from multiple locations with priority:

1. **Project-level** (highest priority): `.grow/agents/**/*.md`, then
   `.claude/agents/**/*.md` — walk
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
│  grow-agent  │  ← This crate
│  (Agent, Builder, │
│   Definition)     │
└────────┬─────────┘
         │ depends on
         ▼
┌──────────────────┐
│  grow-tools  │
│  (ToolBridge,    │
│   ToolRegistry,  │
│   ToolState)     │
└────────▲─────────┘
         │ depends on
┌────────┴─────────┐
│  grow-shell  │  uses AgentBuilder to create
│  (session host)  │  Agent during session setup
└──────────────────┘
```

- **`grow-tools`**: Provides `ToolBridge`, `ToolRegistry`,
  `ToolState`, `SystemReminderLayer`, and tool implementations.
  `grow-agent` depends on it for tool setup.
- **`grow-shell`**: The application shell. Uses `AgentBuilder`
  to construct an `Agent` during session creation. The shell
  re-exports some modules from `grow-agent` (AGENTS.md
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

Unknown frontmatter fields are rejected so misspellings cannot silently change
Agent behavior. Only the explicitly documented Harness/OpenCode compatibility
fields are accepted and ignored.

## Development

```bash
# Check
cargo check -p grow-agent

# Test
cargo test -p grow-agent

# Clippy
cargo clippy -p grow-agent --fix --allow-dirty

# Format
cargo fmt --all
```
