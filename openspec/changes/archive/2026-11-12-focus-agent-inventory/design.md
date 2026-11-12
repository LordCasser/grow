## Decisions

Use the existing live-session discovery contract for primary peers. For subagents, pass the requested scope to canonical history enumeration. Active listing uses the source's admitted Ended facts to suppress known historical tasks, including stale runtime observations. All listing still validates terminal links and returns state without reading result payloads.

The built-in single-result lookup accepts one subagent ID and reuses the session-bound nonblocking observation and canonical result reader. Repeated reads return the same answer when available, or the current observed state and error. This lookup does not consume completion receipts or start waiting for an active child.
