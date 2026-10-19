# Audit: session MCP server identity

## Scope and evidence

This audit records source paths for identity selection and routing. Verification results and build boundaries are recorded in verification.md.

- `crates/codegen/shell/src/session/mcp_catalog.rs`: `mcp_server_key` is the server name for HTTP, SSE, and stdio. The sourced merge seeds configuration, rejects plugin names already claimed by TOML, and inserts plugin entries only when the name is unused. Plugin file entries are collected before inline entries and first-name retention keeps the file definition. The client layer then inserts by name and therefore wins over disk/plugin entries. Different names remain separate even when their endpoints match.
- `crates/codegen/mcp/src/servers.rs`: `validate_mcp_server_names` rejects repeated names in one client-provided catalog before source merging can hide a duplicate. `new_session` and `load_session` call this validator; `extensions/session_admin.rs` validates `session/update_mcp_servers` inputs as well.
- `crates/codegen/shell/src/extensions/mcp.rs`: a direct call with an explicit URL requires the same current configuration entry to match both supplied name and URL. It does not look up a different server by URL. Without an explicit URL, dispatch is by authoritative server name. The call also checks disabled-tool policy before acquiring the client.
- `crates/codegen/shell/src/extensions/mcp.rs`: session `mcp/list` begins with the applicable disk catalog, then overlays the live session snapshot's configs by name. The resulting endpoint is the same endpoint used by session call routing, even when the live client catalog overrides the disk URL.
- `crates/codegen/shell/src/extensions/mcp.rs`, `crates/codegen/shell/src/extensions/session_admin.rs`, and `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs`: setup, toggle, upsert/delete, session updates, new/load, and catalog refresh use the full merge/update path. Session mutation paths hold the session lifecycle lock while applying changes; new/load also coordinate through the catalog reload lock. The update path publishes the client-source input with command admission, before waiting for actor reinitialization, so a concurrent disk reload sees the current client intent.
- `crates/codegen/shell/src/session/actor/mcp.rs`: owned clients, tool registrations, status, and tool prefixes are keyed from the same server name. The archived lifecycle protections ensure obsolete asynchronous work cannot restore a removed or replaced name's previous client/tools.

## Adversarial test checklist

The source review and regression coverage below are complete; see verification.md for exact results and the isolated-build boundary.

- [x] Same-name entries at different endpoints resolve to the existing configuration-source precedence: config over plugin and client over disk/plugin.
- [x] Plugin file and inline entries with the same name retain file-first behavior when their endpoints differ.
- [x] Different names at the same URL both remain present; an HTTP/SSE and stdio entry with one name cannot produce two catalog identities; repeated merge/reload is stable.
- [x] Duplicate names in each client catalog are rejected at new, load, and admin update boundaries with an explicit error.
- [x] A direct call with matching name and URL reaches that entry; mismatched pairs and retired endpoints are rejected without URL-based redirection to another name.
- [x] Session `mcp/list` advertises the live authoritative endpoint when it differs from the disk entry, and its tools route through the corresponding client.
- [x] Setup, toggle, upsert/delete, client catalog updates, and concurrent config reload preserve one name-to-client-to-tool-prefix identity under the session lifecycle lock.
- [x] Cold recovery and hot replacement do not publish or dispatch through a stale endpoint after a same-name replacement.

The combined verification evidence is **306 passing related tests** (169 MCP core, 34 Pager, 103 Shell), with the current workspace planning build limitation described in verification.md.
