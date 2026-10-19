# Design: Unify session MCP server identity

## Context

The archived lifecycle contract makes the session's effective catalog authoritative for tools and calls, including explicit endpoint checks and cold recovery. That contract relies on server identity being stable across configuration layers, client instances, and routing inputs. The audit confirmed that the merge used URL keys for HTTP/SSE while clients and tool prefixes used name keys; URL-only direct-call fallback also mixed two identities.

## Decisions

- Within one session, `name` is the identity shared by the catalog directory, its client, and the tool-name prefix.
- Preserve the existing source precedence: configuration over plugin, and client configuration over directory configuration where the established merge rule applies.
- For same-name entries across configuration layers that refer to different endpoints, choose one authoritative entry deterministically according to that precedence. Never combine a name from one source with a client/endpoint from another.
- A single client-provided server array containing duplicate names is invalid and is rejected as a whole. `session/new`, `session/load`, and explicit catalog update return ACP invalid-params before mutating a session or admitting initialization.
- A direct call with both name and URL is valid only when both identify the same current authoritative catalog entry. A matching URL cannot redirect a mismatched name to another server.
- Cold recovery reconstructs the same authoritative identity from current applicable inputs. Hot replacement updates the name's client and tools as one identity transition; stale work cannot route a call to the previous endpoint or publish it as current.

## Validation approach

Trace configuration and plugin merge, client-array validation, catalog directory insertion, tool prefix construction, direct-call lookup, session recovery, and replacement paths. Add adversarial coverage for duplicate names within one client array, same-name cross-layer entries on distinct endpoints, explicit mismatched name/URL pairs, and calls during cold recovery/hot replacement. Record source evidence and any contract/implementation discrepancy before changing code.

## Implementation

The sourced and final merge maps now both use server name for every transport; final ordering is stable by name. Existing config-over-plugin and client-over-config insertion precedence chooses one slot. Different names at the same URL remain separate slots (including separate credentials/clients); URL similarity is not authority to alias them. Plugin file-over-inline handling remains unchanged.

`validate_mcp_server_names` validates each standard ACP client array before merging, including resident/cold load and explicit update. Direct invocation looks up only the supplied name and verifies any supplied URL against that name's authoritative entry; URL-only fallback is removed. The existing generation/episode guards retire old asynchronous work on a real replacement.

The audit also found that `mcp/list` annotated a disk URL with a different client-owned session's readiness, and configuration mutation handlers installed disk entries directly or tore down by name. List projection now overlays current session configs before status annotation; setup/toggle/upsert/delete commit a complete merged catalog through `UpdateMcpServers`, under the existing session lifecycle lock. This keeps UI mutation, watcher refresh, client intent, and displayed endpoints on the same authority.
