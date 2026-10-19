# Tasks

- [x] 1.1 Trace the accepted catalog refresh, config diff, tool bridge, MCP reconnect, and cold session recreation paths; record concrete gaps in this change before implementation.
- [x] 1.2 Ensure removal and replacement converge advertised tools, callable tools, and owned connections on the current effective catalog.
- [x] 1.3 Ensure stale initialize/reconnect completions cannot restore removed or replaced configuration.
- [x] 1.4 Verify catalog updates remain isolated across sessions with distinct cwd values.
- [x] 1.5 Verify cold recovery uses current applicable configuration, preserves client-source behavior supported by session recovery, and reinstalls enabled project watching.
- [x] 1.6 Run targeted lifecycle regressions and strict OpenSpec validation; record evidence before completing tasks.
