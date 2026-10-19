# MCP catalog lifecycle adversarial audit

Date: 2026-10-19. Scope: source-only changes; no installed binary replacement, real-session restart, user configuration edit, or request to a configured user MCP/model service.

## Authority and flow

`pager::acp::spawn` owns the in-process config watcher/reloader and its cancellation. The watcher uses the same `RepoDirChain` as project config discovery. `reload_mcp_catalog` selects applicable resident session handles and merges disk/plugin configuration with each handle's latest client input. The actor commits `McpState` diffs, retires removed names, and initializes the new catalog. Client notifications enter the session-owned dispatcher through client/config/transport episodes. `ToolBridge` definitions feed the MCP search snapshot and status listing; calls use the session's current `McpState`.

Cold `session/load` creates a new runtime using the current disk configuration and the client catalog supplied by that load request. Existing persisted session content does not serialize transport handles or a client catalog. A resident load also replaces its client input when the input is empty.

## Findings fixed

| Finding | Consequence | Change and evidence |
| --- | --- | --- |
| Resident load ignored an empty effective catalog | Old client-only service survived reconnect and later disk refresh | Unconditional resident replacement; integration RED reproduced retention before fix |
| Explicit client update saved its source after initialization acknowledgement | Concurrent reload could merge the old client set | Publish source immediately after command admission; deterministic paused-ack test |
| List notifications/recovery status did not rebuild registrations | Listed/model-search tools and schemas stayed stale after server changes | Episode-gated per-server discovery owned by dispatcher; schema, empty catalog, recovery and auto-restart-off tests |
| Retirement removed bridge entries but left search snapshot | A rebuilt bridge could restore removed definitions | Prune both synchronously, cancel superseded init, generation-check snapshot commit; actor restore test |
| Retired explicit URL fell back to a same-name entry | Call could reach a replacement endpoint | Reject absent explicit endpoint; wire counters prove neither endpoint called |
| A retired client's late tool failure could recover/retry it | Old connection could be revived by an old request | Check captured Arc authority before recovery and retry; deferred-error removal/replacement tests |
| Watch scope omitted repository ancestors read by merge | Nested sessions missed root config changes and late `.grow` creation | Watch the consumed directory chain; shared-ancestor ownership plus two-session CRUD tests |

Initialization and stdio recovery already have generation/config/episode admission checks; the audit retained them and added blocked-handshake and late-discovery fixtures rather than adding another identity system. Dispatcher catalog jobs use an owned JoinSet: newer jobs replace only their server's job; config removal aborts its job; dispatcher drop aborts outstanding discovery.

## Adversarial boundaries

- Two session cwds use independently owned clients, including equal server names. Global changes reach both; project-local changes reach only applicable sessions. Tests verify returned endpoint identity and wire invocation counters.
- A blocked old initialize and a blocked old tools/list result cannot overwrite the replacement or revive a deleted server. Removed definitions cannot return through local bridge restore.
- A server may report zero tools. Status JSON omits an empty vector; the regression treats an omitted `tools` field as empty only after confirming the server is ready. This was a test-fixture assertion correction, not a runtime workaround.
- `grow/mcp/call` is a raw diagnostic RPC, not the model tool registry whitelist. A retired tool name may be sent to the current server and rejected there. Tests separately prove model ToolBridge rejects old definitions, and removed servers/retired explicit URLs do not produce wire calls.
- Already-dispatched calls may settle on their original peer. Removal does not undo remote effects; it prevents stale recovery/retry and new routing through the retired catalog.
- With the existing event dispatcher disabled (`liveness_watchers=false`), server-origin notification refresh is not provided. `auto_restart=false` disables transport recovery while catalog discovery remains enabled in a running dispatcher.
- Remote discovery failures retain the last successfully discovered catalog and log the bounded failure; no new retry/failure policy was introduced.

## Separate architectural debt

Auxiliary per-tool policy reload and duplicate-name catalog identity have different ownership/contract questions from server lifecycle. Concrete evidence is recorded in `openspec/backlog.md`; neither is silently claimed fixed by endpoint lifecycle tests. No merge-key or per-tool policy framework was added to this change.
