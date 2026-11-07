# SDK producer reverse-routing verification

## Scope

This verification covers the leader-side SDK reverse-routing fixture only. It exercises the real `GatewayAcpInvoker`, the real leader socket path, and real `ClientMessage::Acp` replies. It does not change leader production logic.

The fixture establishes B as `sess-a`'s driver by completing `session/load`, then completes A's later `session/new` for the same session. A's accepted connection id is taken from the actual leader-forwarded `_meta.grow/leaderClientId`; the test does not manufacture the registration owner. A later generic reverse probe is delivered to and answered by B, proving A's new response did not steal B's driver role.

For both A and B, using the same `serverId`, the test sends `initialize`, `tools/list`, and `tools/call` through `GatewayAcpInvoker`. Each request is observed on the owning local socket, checked for session id, accepted owner id, server id, and unchanged inner MCP JSON value, then answered from that socket. The leader returns the outer ACP response to a pending-id dispatcher, which completes the original invoker with the socket-supplied inner MCP response. Outer ACP ids and inner MCP ids are asserted separately.

After A disconnects, the test first observes `grow/internal/queue_client_disconnected` for A's accepted id. A subsequent real SDK request receives no socket reply and ends through the invoker's existing 250 ms timeout. A direct next-payload causal barrier proves B did not receive a fallback SDK request.

## Commands and results

All Cargo commands used the required non-incremental environment (`CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, `RUST_MIN_STACK=16777216`, pinned Rust 1.93.1, locked and offline).

- Minimum real round trip: `cargo +1.93.1 test -p shell --lib --locked --offline sdk_producer_reverse_targets_registration_owner_not_driver`
  - Task: `01a1b343-c1f3-7031-8e16-3d79ac441d27`
  - Log: `/tmp/grow-sdk-registration-leader-real-1.log`
  - Exit 0; 1 passed, 0 failed.
- Final A/B matrix and disconnect case: same focused command.
  - Task: `01a1b34f-1dd3-7380-a875-5936070603c9`
  - Log: `/tmp/grow-sdk-registration-leader-real-2.log`
  - Exit 0; 1 passed, 0 failed; test body completed in 0.34 s.
- Target/unicast and disconnected-target regression: `cargo +1.93.1 test -p shell --lib --locked --offline leader_client_id_`
  - Task: `01a1b354-07e8-76d0-aacd-ebc557c6c680`
  - Log: `/tmp/grow-sdk-registration-leader-route-regress-1.log`
  - Exit 0; 5 passed, 0 failed.
- Driver join-not-steal regression: `cargo +1.93.1 test -p shell --lib --locked --offline two_clients_one_session_broadcast_and_driver`
  - Task: `01a1b354-894f-75e2-b856-7ab9a2adbdcf`
  - Log: `/tmp/grow-sdk-registration-leader-driver-regress-1.log`
  - Exit 0; 1 passed, 0 failed.
- `rustup run 1.93.1 rustfmt --edition 2024 --config skip_children=true --check crates/codegen/shell/src/leader/server.rs`
  - Exit 0.
- `git diff --check -- crates/codegen/shell/src/leader/server.rs`
  - Exit 0.

The Cargo runs emitted one pre-existing `irrefutable if let` warning in `session/actor/input_admission.rs`; it is outside this change.

## Coverage limits

The standalone `owner=None` producer control in `session/acp_mcp.rs` was not run here. Real MVP new/resident/cold lifecycle and inheritance scenarios are a separate milestone and are not claimed by this verification. No workspace-wide suite, soak test, OpenSpec archive, release, commit, or push was performed.
