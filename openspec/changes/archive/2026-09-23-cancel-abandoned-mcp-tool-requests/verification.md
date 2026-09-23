# Verification record

## Baseline evidence

- rmcp 2.2 `service.rs::PeerRequestOptions::no_options` has `timeout: None`; `RequestHandle::await_response` therefore waits for a reply without its own timeout notifier. The `with_timeout` branch sends a cancellation asynchronously before returning, so using it with a Drop guard risks duplicate notification if the future is dropped mid-send.
- `call_tool_cancel_aware` applies one absolute deadline to `send_cancellable_request` and `await_response`. The guard exists only after a request id is returned and owns the originating peer; the existing HTTP timeout path still conditionally resets that same service and does not retry timed-out tools. The direct entry includes initialization in its original total budget.
- `mcp/src/acp_transport.rs` currently discards id-less outbound notifications, so ACP reverse bridge delivery is not asserted.

## Executed checks

| Command | Exit | Result |
| --- | ---: | --- |
| `openspec validate cancel-abandoned-mcp-tool-requests --strict --no-interactive` | 0 | Change schema and scenarios valid. |
| `git diff --check -- crates/codegen/mcp/src/servers.rs crates/codegen/shell/src/extensions/mcp.rs docs/development.md` | 0 | No whitespace errors in touched source/docs. |
| `cargo test --locked -p mcp --lib mcp_projection_ -- --nocapture` | 101 | Cargo refused: lock file needs an update. No lock update or unlocked retry was attempted. |
| `cargo test --locked -p mcp --lib mcp_tool_call -- --nocapture` | 0 | After keeping the originating `McpService` alive through send, completed/timeout/drop tests passed 3/3. The first run exposed a real notification-before-transport-drop race and was fixed; the second run's exact-id/one-notification assertions passed. |
| `cargo test --locked -p mcp --lib direct_mcp_call_uses_same_deadline_and_cancellation -- --nocapture` | 0 | Direct deadline and matching request id passed 1/1. |
| `cargo test --locked -p mcp --lib abandoned_mcp_call_never_cancels_replacement_service -- --nocapture` | 0 | Two duplex services: abandoned old call sent one cancellation to old peer/id, none to replacement; a later round sent one cancellation to replacement peer/its own id. Initial assertion incorrectly counted the replacement's handshake notification; corrected to classify notification methods, then passed. |
| `cargo test --locked -p mcp --lib timeout_and_future_drop_race_sends_only_one_cancellation -- --nocapture` | 0 | Eight duplex timeout/drop races: exactly one matching-id cancellation per in-flight request, no duplicate. |
| `cargo test --locked -p mcp --lib cancellation_send_failure_does_not_block_drop -- --nocapture` | 0 | Closed peer rejected notification; dropping cancellation guard completed within 100 ms without propagating the send failure. |
| `cargo test --locked -p mcp --lib try_call_tool_http_mcperror_recovers_then_retry_succeeds -- --nocapture` | 0 | Existing HTTP recovery path still performs the sole allowed retry after a recoverable JSON-RPC error. |
| `openspec validate --all --strict --no-interactive` | 0 | 19 current specs/changes passed, 0 failed; archive and archived-spec revalidation remain pending. |

## Known scope limit

- `Cargo.lock` was reconciled by the parent task. Main call, direct call, completion, timeout, drop, two peer identities, failed notification, timeout/drop race, and existing HTTP retry now have passing focused tests.
- ACP reverse bridge still discards id-less outbound notifications; no remote-delivery claim or bridge-level test is made. This remains a separate backlog item.

## Archive integration

- `openspec validate --all --strict --no-interactive`: exit 0, 19/19 before archive and 18/18 after archive.
- `openspec archive cancel-abandoned-mcp-tool-requests --yes`: exit 0; added one `extension-runtime` requirement under `2026-09-23-cancel-abandoned-mcp-tool-requests`.
- The first `openspec validate --all --strict --no-interactive --archived` correctly reported this just-archived change's archive task as incomplete (366/367); task 3.3 was checked only after the command had actually run. The rerun passed 367/367 (exit 0).
