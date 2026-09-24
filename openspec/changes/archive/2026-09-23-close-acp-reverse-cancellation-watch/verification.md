## Review

- `openspec/specs/extension-runtime/spec.md` scopes the cancellation guarantee to stdio/Streamable HTTP and its `ACP reverse bridge limitation` scenario explicitly forbids claiming that the ACP-hosted remote server received cancellation.
- `crates/codegen/mcp/src/acp_transport.rs` documents the reverse transport as half-duplex and discards JSON-RPC id-less messages, which includes `notifications/cancelled`.
- `crates/codegen/shell/src/session/acp_mcp.rs` sends one `grow/mcp/sdk_call` reverse request and waits for its response; it has no independent path for server-originated notifications.
- `docs/development.md` states the same stdio/HTTP-only delivery guarantee and warns that notification delivery does not prove remote side effects were revoked.
- Therefore this item is an optional transport expansion explicitly outside the accepted contract, not an unfulfilled correctness requirement. Only its active backlog bullet is removed.

## Validation

- `openspec validate 2026-09-23-close-acp-reverse-cancellation-watch --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed, 16/16 active items before archive.
- `git diff --check` — passed.
