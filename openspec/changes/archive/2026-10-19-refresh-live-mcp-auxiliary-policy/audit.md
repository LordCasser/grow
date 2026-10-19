# Audit: live MCP auxiliary policy

## Scope and evidence

This audit records the current source paths relevant to refreshing an existing session's MCP policy. Verification results and build boundaries are recorded in verification.md.

- `crates/codegen/shell/src/config/reloader.rs`: global reload compares `mcp_servers`, `disabled_mcp_tools`, and `mcp`, then emits `McpCatalogChanged`; project configuration changes are dispatched separately with a project root.
- `crates/codegen/shell/src/session/actor/run_loop.rs`: the `UpdateMcpServers` command reseeds the MCP output cap and calls `refresh_mcp_auxiliary_policy` before checking whether the endpoint catalog diff is empty. A policy-only update therefore still runs when server endpoints are unchanged.
- `crates/codegen/shell/src/session/actor/mcp.rs`: the actor reads disabled tools for its actual `session_info.cwd`, refreshes policy only on `owned_clients`, replaces the actor's disabled-tool map, stashes and unregisters newly disabled bridge tools, restores registrations when re-enabled, then refreshes its model search snapshot and emits changed status. It does not perform a remote tools/list for this policy-only path.
- `crates/codegen/mcp/src/servers.rs`: each client has an `Arc<RwLock<McpClientPolicy>>` separate from its transport state. `refresh_policy` replaces the effective policy without resetting transport. Metadata values take priority over host overrides for the same field; per-tool entries take priority over server defaults. Removed overrides fall through to the remaining source or the defined default. Millisecond metadata is rounded up to seconds.
- `crates/codegen/shell/src/session/mcp_servers.rs`: project timeout configuration is consulted only when `project_scope_allowed(cwd)` is true. `util/config/mcp.rs` merges disabled-tool project tables merged only for trusted project scope; project entries replace same-name global entries, and an empty array is filtered into no disabled set, exposing the tool.
- `crates/codegen/mcp/src/servers.rs` and `crates/codegen/shell/src/extensions/mcp.rs`: model tool dispatch and direct `grow/mcp/call` admission both check the current disabled-tool set before invoking a client. The direct call uses the client's current per-tool timeout.
- `crates/codegen/shell/src/session/actor/mcp.rs`: clients that finish initialization after a policy reload are refreshed from current cwd/config immediately before insertion. The stdio respawn path likewise resolves policy again after its handshake and before installing the replacement client.
- `crates/codegen/mcp/src/servers.rs`: ACP reverse calls read the live backstop from the policy lock when each request is dispatched. The backstop ceiling only grows across policy refreshes, while the call's outer deadline uses the timeout selected at admission. A shorter policy therefore affects new calls without shortening an already admitted call.
- `crates/codegen/shell/src/session/actor/run_loop.rs`: subagent policy refresh reseeds that actor's output cap and invokes the same auxiliary refresh. Since the refresh iterates only its `owned_clients`, it updates the subagent's cwd-scoped disabled mask and locally owned clients without changing inherited/shared parent clients.

## Adversarial test checklist

The source review and regression coverage below are complete; see verification.md for exact results and the isolated-build boundary.

- [x] Global `mcp_servers`, `disabled_mcp_tools`, and `[mcp]` changes reach an existing session even when its server endpoint vector is unchanged.
- [x] Trusted project timeout, per-tool timeout, startup timeout, and disabled-tool values override global values for the matching cwd; an empty disabled array removes the inherited global mask.
- [x] An untrusted project cannot change another session's effective timeout or disabled-tool policy.
- [x] Removing a project override restores global values and the global disabled mask.
- [x] A disabled tool is absent from the model bridge and search snapshot; model and direct calls fail without reaching the mock server; a tools/list refresh cannot restore it while disabled.
- [x] Re-enabling a tool restores a callable registration and keeps bridge definitions and search snapshot in agreement.
- [x] A policy change while a call is in flight leaves its admitted outer deadline intact; a subsequent call uses the new timeout, while the ACP reverse backstop does not shrink below its prior ceiling.
- [x] Pending initialization and stdio respawn install clients with the latest policy; a subagent refresh leaves inherited parent clients unchanged.

The combined verification evidence is **306 passing related tests** (169 MCP core, 34 Pager, 103 Shell), with the current workspace planning build limitation described in verification.md.
