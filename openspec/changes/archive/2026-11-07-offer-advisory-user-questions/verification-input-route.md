# U0 route fix: `grow/input/admission_status` in the top-level ACP dispatch — 2026-11-06

Fixes `review-reply-input.md` U0: `extensions/session_admin.rs::handle` already
implemented the method (typed bounded shared read, camelCase request, exact
identity echo), but `agent/mvp_agent/acp_agent.rs`'s top-level `ext_method`
whitelist omitted it, so public ACP callers got
`Method not found: unknown ACP extension method: grow/input/admission_status`
(wire evidence `/tmp/grow-ask-pty-casts/advisory_initial_reply_unknown.rpc.log:19–20`).

## Production change (one line, one file)

`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs`: added
`"grow/input/admission_status"` to the existing `session_admin::handle` method
arm (alongside `grow/queue/prompt_status` etc.). No new handler, no wildcard
`grow/*` fallback, no change to `session_admin` semantics (existing session
without a durable record still returns `unknown_unconfirmed`; only a missing
session returns `absent`; empty/oversized ids still `invalid_params`), no new
public entity.

## Red → Green (real public ACP entry, no direct handler calls)

Test target: `crates/codegen/shell/tests/test_advisory_question_admission.rs`
(owned from prior rounds), extended with a route-probe section after scenario D
using the same live `MvpAgent` / `connect_agent_v1` / `connect_client_v1`
duplex connection and the still-alive session. Probes call
`client_conn.ext_method(ExtRequest::new("grow/input/admission_status", …))`.

- **Old red** (`/tmp/grow-ask-input-route-oldred.log`, exit 101): with the
  pre-fix source the first probe failed with
  `code: -32601: Method not found, data: "unknown ACP extension method:
  grow/input/admission_status"` — a genuine routing failure, matching the
  captured wire evidence.
- **Green** (`/tmp/grow-ask-input-route-green.log`, final run exit 0,
  `1 passed; 0 failed`): three probes over the fixed route —
  1. same alive session, stable never-Submitted id
     `question-reply:{session}:call-advisory-1` → `status.status ==
     "unknown_unconfirmed"`, `sessionId`/`promptId` exact echo,
     `source == "durable"` (the observation enum is internally tagged on
     `status`, so the wire nests one level; the first green attempt asserted
     the un-nested path and failed as a fixture-shape error — fixed by
     matching the actual typed response, no production change, no semantic
     relaxation);
  2. missing session id → `status.status == "absent"`, `source == "durable"`
     (existing Absent contract preserved);
  3. empty `promptId` → JSON-RPC error code `-32602` (invalid_params from the
     handler, not method-not-found).
  The pre-existing four model-admission scenarios (advisory Presented,
  setter-forced blocking, re-enable, typed-policy negative) all still pass in
  the same test run.

Adjacent route regressions in the green log: `queue_control` 3 passed,
`input` 165 passed, `prompt_status`/`admission_status` 0-matched filters
trivially ok — all exit 0.

## Command

```
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 PATH=~/.cargo/bin \
  cargo +1.93.1 test --locked --offline -p shell --test test_advisory_question_admission
```

Logs: `/tmp/grow-ask-input-route-{oldred,green}.log` (full output, not
tail-truncated; each cargo exit recorded).

## Scope / release

Write scope used: `acp_agent.rs` (one-line route) and the route-probe section
of my own admission integration test. Isolation unchanged (tempdir GROW_HOME,
test key, loopback mock); no real user state; no Pager/UI work (U1/U2/U3
remain with the parent); no fmt/commit. All background cargo tasks exited
normally; **Cargo window and acp_agent/test write scope are released**. This
U0 fix is the endpoint of this item.
