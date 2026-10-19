# Design

`handle_reload_mcp_catalog` captures root and active-child `SessionHandle`s before sending commands, filters both by the requested project root using each session's own cwd, and deduplicates session IDs during lifecycle handoff. Root sessions keep the existing client-intent merge path. Child sessions receive `UpdateMcpServers` with an empty server list: the child run loop treats this command as policy-only, recomputes its cwd-scoped auxiliary gate, and discards catalog replacement input. This keeps parent-owned shared clients and child frozen catalog state untouched.

The command is the existing owner boundary for policy refresh; the handler does not inspect or mutate child actor state directly.
