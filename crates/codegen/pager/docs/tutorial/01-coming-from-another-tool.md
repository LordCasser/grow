# Bringing existing project context

Grow deliberately uses one canonical configuration model. Existing material can be migrated explicitly instead of being scanned through a second runtime path.

## Canonical destinations

- **Directory instructions:** `AGENTS.md`
- **Rule collections:** `.grow/rules/*.md`
- **Skills:** `.grow/skills/<name>/SKILL.md`
- **User-invocable command Markdown:** `.grow/commands/*.md`
- **MCP servers:** `[mcp_servers.<name>]` in `~/.grow/config.toml` or project `.grow/config.toml`
- **Hooks:** `$GROW_HOME/hooks/*.json`, project `.grow/hooks/*.json`, or the native `[hooks]` TOML section
- **Permission rules:** `[permission]` in Grow configuration
- **Agent roles:** `.grow/agents/**/*.md`

Copy the source content once, translate it to the canonical schema, then remove the old source from the Grow workflow. There is no background vendor-directory scan or import marker.

Run `grow inspect` inside the repository to verify the exact instructions, skills, hooks, Agents, plugins, and MCP servers that the session will see.

Other useful surfaces: `/btw` asks a side question without interrupting the current task, `/rewind` restores file snapshots and conversation state, and the Trajectory page exposes the durable Timeline used for recovery.

*Go deeper: `/docs Project Instructions (AGENTS.md)`, `/docs Skills`, or `/docs MCP Servers`*
