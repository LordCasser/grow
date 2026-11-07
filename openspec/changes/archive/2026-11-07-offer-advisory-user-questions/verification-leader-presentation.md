# U4 leader fix: advisory ask routed driver-only, out of the shared interaction cache — 2026-11-06

Fixes `review-leader-presentation.md` U4: `is_interaction_request` classified every
`grow/ask_user_question` reverse-request as a shared blocking interaction. The advisory
worker never emits the `InteractionResolved` broadcast the shared cache's eviction relies
on, so a settled advisory presentation stayed cached and was replayed to later attachers.

## Production change (one function, one file)

`crates/codegen/shell/src/leader/server.rs::is_interaction_request`:

- `session/request_permission` and `grow/plan_approval` — unchanged, always shared.
- `grow/ask_user_question` — shared **unless** the unwrapped inner params (via the
  existing `interaction_inner_params`, so direct and gateway-wrapped forms classify
  identically) carry an explicit boolean `blocking == false`. Only that value flips it:
  missing (serde default `true`), explicit `true`, and malformed non-bool values all
  remain shared. Explicit-false asks now take the existing driver-only reverse-request
  route (no new path, no fake `InteractionResolved`, no cache manager, no broadcast
  changes). Other clients rebuild advisory cards from normal tool-history updates/replay;
  the presentation result is still judged only by the driver's real `Presented`.

Nothing else in `server.rs` was touched.

## Red → Green (real leader event loop, real sockets)

Test additions in `server.rs`' test module (same file, allowed scope):

- Helper: `is_interaction_request_routes_explicit_advisory_as_driver_only` —
  wrapped/direct `blocking:false` → driver-only; missing/`true`/`"false"`(string)/`0`
  in both wire forms stay shared; `request_permission` ignores a stray `blocking` field.
- Socket: `advisory_ask_routes_driver_only_and_is_not_cached_for_replay` — uses the real
  `setup_persistent_server_with_agent` + `connect_and_register`/`load_session`/
  `complete_load`/`next_acp_payload_matching` fixtures: driver A + subscriber B attached
  to one session; a wrapped `blocking:false` ask is received by the driver only (bounded
  negative window proves B never gets it), the driver's `Presented` reply is forwarded
  back to the agent, a late joiner completing `session/load` does NOT receive the settled
  advisory (bounded negative window), then a default-blocking ask (no `blocking` field)
  still broadcasts to both A and B and is evicted from replay by `InteractionResolved`
  (a fourth client sees neither).

**Old red** (`/tmp/grow-ask-leader-oldred.log`): against the unfixed source the socket
test failed at "an advisory ask must NOT be broadcast to other subscribers" and the
helper at its first explicit-false classification — genuine routing failures, exit 101
each.

**Green** (`/tmp/grow-ask-leader-green.log`, final section `GREEN final rerun`):
socket `1 passed` exit 0; helper `2 passed` exit 0. (Two earlier helper failures were
test-fixture JSON brace errors in my new strings — trailing close brace — fixed in the
fixture only; no assertion semantics changed, no production change. The socket test was
green in all three runs against the fixed source.)

**Regressions** (`/tmp/grow-ask-leader-regress.log`, all exit 0): shared-interaction
semantics preserved — `interaction_request_broadcasts` 1, `pending_interaction_replayed`
1, `pending_child_interaction` 1, `interaction_raised_with_no_subscriber` 1,
`resolved_interaction_not_replayed` 1, `pending_interaction_survives_disconnect` 1,
`extract_interaction` 2.

## Command

```
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 PATH=~/.cargo/bin \
  cargo +1.93.1 test --locked --offline -p shell --lib [-- --nocapture <filter>]
```

Logs: `/tmp/grow-ask-leader-{oldred,green,regress}.log`, full output with per-command
exit codes recorded.

## Limits

- Evidence is the in-process leader event loop over real local sockets (module-level
  integration); fresh-CLI/PTY end-to-end and the independent review remain with the
  parent, per the review plan. No Pager/U2 sources, no ACP-1 capacity or other MCP
  changes, no real leader/user config/session touched; tempdir socket/home isolation
  only. No fmt/commit.
- All background cargo commands have exited; **Cargo window and server.rs write scope
  are released** (available for U5).
