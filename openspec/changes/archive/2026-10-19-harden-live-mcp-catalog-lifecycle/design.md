# Design: Harden live MCP catalog lifecycle

## Context

The accepted `extension-runtime` contract already requires live session refresh on applicable `McpCatalogChanged` updates, actual session cwd scoping, client precedence for the same merge key, idempotent identical updates, and cancellation of the in-process watcher. `configuration-rules` defines where user and project configuration are discovered, but does not define how a running MCP catalog transitions when a source removes or replaces an entry.

MCP server configuration is merged into an effective catalog before `SessionCommand::UpdateMcpServers` updates the session state. Current source also has typed config diffs, tool removal for removed servers, MCP initialization/recovery paths, and session restart paths. The audit found gaps at client-source admission, catalog notification consumption, retirement snapshots, and project-watch scope. The fixes retain the existing merge, command, event-episode, and recovery owners.

## Decisions

- Treat the effective merged catalog as the authority for both advertised tools and invocation. After a removal or replacement settles, an old connection or tool registration cannot remain usable through stale session state.
- Treat each applicable configuration update as superseding earlier asynchronous initialize/reconnect work for the replaced or removed server. A stale result cannot restore old configuration, tools, or readiness.
- Keep catalog scope session-specific: global configuration applies according to existing merge behavior; project configuration is limited to sessions whose cwd resolves within its applicable project scope; one session's update cannot mutate another session's catalog.
- On cold recovery, rebuild the effective catalog from current applicable configuration and existing session-held client-source inputs, preserving established trust filtering and merge precedence. Re-establish project watching only when the existing setting enables it. The client catalog comes from the current `LoadSessionRequest.mcp_servers`, not persisted connection state.
- Keep transport recovery, user authorization, trust filtering, and foreground/session policy unchanged.

## Confirmed implementation gaps and changes

- Resident `session/load` skips catalog updates when the merged vector is empty. Always apply the replacement client source and effective catalog, including an empty set.
- The explicit client catalog update publishes its client source only after awaiting actor reinitialization. Publish the source with command admission, before yielding, so a concurrent disk reload cannot reconstruct the superseded client set.
- `ToolsChanged` events and HTTP/stdio recovery status currently do not rebuild ToolBridge registrations or search metadata. Reuse the existing dispatcher and host actions to own cancellable per-server catalog refresh jobs. Retire obsolete jobs on a newer event/config removal; check the captured client episode before and after remote discovery. Keep catalog refresh available when automatic transport recovery is disabled.
- Removing/replacing a server unregisters its bridge prefix but leaves the search snapshot until the next initialization finishes. Prune both immediately, cancel superseded initialization work, and reject obsolete snapshot commits so bridge reconstruction cannot resurrect removed tools.
- Calls already dispatched may settle on their original connection. Catalog changes do not authorize an old call to reconnect or retry a retired client, or retarget a replacement connection.
- A direct request containing a retired explicit URL currently falls back to the server name, potentially invoking a replacement endpoint. Require the explicit endpoint to remain configured.
- The catalog merge reads `RepoDirChain` ancestors, while dynamic project watches only cover the leaf cwd. Reuse the same directory-chain resolver for non-recursive watches and retain a shared ancestor until all registered cwd owners release it.

The existing client input from `session/load` is the recovery source; do not persist transport state or invent a durable client catalog.

Source and deterministic tests should exercise remove, replace, and re-add while initialization or reconnect is in flight; list-versus-call agreement; two sessions with different cwd; and cold recovery after configuration changes while stopped. Verify client-source behavior using the session's existing capture/recovery semantics rather than inventing a new persistence source. Record uncovered implementation gaps instead of weakening these invariants.
