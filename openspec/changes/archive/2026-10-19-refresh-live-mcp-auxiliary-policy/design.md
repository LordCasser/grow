# Design: Refresh live MCP auxiliary policy

## Context

The archived live catalog lifecycle change establishes the effective catalog as the authority for a session and prevents older initialize/reconnect work from restoring retired server state. Per-server policy is adjacent state: it can change while the server identity and endpoint remain the same, and must be applied without widening the change into unrelated catalog behavior.

## Decisions

- Apply the currently effective policy to an already-existing session when applicable configuration changes any of `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, or `tool_timeouts`.
- Interpret removal of a policy override as exposing the next lower-priority setting or the normal default for that field, rather than retaining the prior effective override.
- Preserve the established metadata precedence when effective policy is assembled; `_meta` overrides host config for the same field, while per-tool entries override the server default, and removed host entries expose the remaining lower-priority/default value.
- Scope policy refresh by the existing configuration merge, trust filtering, and session cwd rules. A project policy that is not trusted or applicable to a session cannot affect that session.
- Treat policy changes as superseding earlier asynchronous initialization/reconnect work. Such work cannot commit policy captured before the newer effective configuration.
- A call's timeout/deadline is selected at admission. A later policy update affects subsequently admitted calls and does not retroactively extend or shorten an in-flight call.
- Model tool admission captures the timeout and absolute deadline before awaiting client initialization. The same deadline bounds initialization and the remote tool call.
- A subagent handles catalog-update notifications only as a trigger to refresh cwd-scoped auxiliary policy. It ignores the incoming server vector and refreshes its own disabled gate/registrations; timeout changes apply only to clients it owns, never inherited shared client Arcs.

## Validation approach

Trace policy loading, merge, session update, connection initialization, timeout selection, and recovery paths before implementation. Exercise changed and removed overrides, metadata precedence, trusted and untrusted project sources across different cwd values, stale asynchronous completion, and an in-flight call crossing a policy update. Record any mismatch between current implementation and these scenarios in the change before modifying code.

## Implementation

Global reload compares `mcp_servers`, `disabled_mcp_tools`, and `mcp`. The actor refreshes auxiliary policy before the unchanged-server early return. Trusted project masks overlay global masks by server name; an empty array explicitly clears an inherited mask. Untrusted project timeout/mask input is excluded by the existing folder-trust decision.

`McpClient` keeps one shared effective policy value separately from transport ownership. Refresh replaces its fields under a short synchronous write lock. Tool admission copies its timeout before IO; startup/recovery uses the policy present when it begins. This avoids reconnecting an unchanged endpoint for policy-only changes. For ACP reverse RPC, the discovery/call backstop is read for each request and its ceiling can grow, while never shrinking below earlier admitted budgets. The outer MCP call remains the actual timeout owner, so shorter new call budgets still take effect.

The actor stashes disabled bridge registrations and restores enabled ones using its existing registration path, then refreshes the search snapshot and complete UI tool list. Runtime model and direct-call admission also check the mask; a retained old registration and a recoverable old failure cannot bypass it. A pending handshake reconciles current policy before publishing its previously unowned client, and stdio respawn does the same. Recovery of the same client does not overwrite policy. Subagent update notifications use this local reconciliation path without replacing the child catalog or mutating parent-owned shared-client policy.

Subagent catalog update commands retain their existing prohibition on replacing inherited client catalogs, but still refresh the local disabled mask. Shared parent clients are never rewritten by the child refresh; owned clients remain scoped to their owner. A call captures its deadline before waiting for initialization, so a policy-only edit cannot alter an admitted call's budget during that wait.
