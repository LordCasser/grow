# grow-test-support

Shared test infrastructure for the grow-build crates: mock inference server,
SSE wire-format generators, ACP stdio clients, headless runner, and the shared
`TestSandbox` filesystem/environment plus `TestProcess` subprocess owners. PR3
owns test subprocesses only; production spawning, leader protocol, and startup
behavior are unchanged. Consumed by `grow-shell`
integration tests, `grow-pager-pty-harness` (`ContentController`), and
`grow-sampler` tests.

> **Freshness rule:** update this README in the same PR that changes `src/` —
> reviewers should treat a `src/` diff without a README diff as incomplete.

How-to-test discovery lives with the pager PTY harness crate
(`grow-pager-pty-harness`). This file is the API reference for the shared
test-support surface.

## Module map

| Module | What it provides |
|--------|------------------|
| `inference_override` | Typed request matching and response precedence shared by all inference routes: endpoint + foreground/auxiliary classification, named expectation state, overlapping-duplicate fingerprint replay, per-expectation barriers, compatibility FIFO dispatch, auth rejection, and compatibility completion-gate policy. The module is crate-private; only `InferenceEndpoint`, `InferenceRequestMatcher`, and `InferenceExpectation` are re-exported. |
| `mock_server` | `MockInferenceServer` serves the three inference backends plus `/v1/models`, `/v1/settings`, a minimal `/v1/user`, and the local privacy test route on `127.0.0.1:0`. Inference precedence is **matched expectation > compatibility FIFO > required-auth > echo/fixed mode**. Named expectations support deterministic overlapping-request replay and completion barriers. Settings are 404 until set via `set_settings`; `preset_settings_empty` installs an empty successful response. Request logging exposes bodies, authorization, headers, endpoint counts, the last system prompt, and a concise summary. Runtime knobs include `set_models`, `set_messages_stop_reason`, and fixed/echo responses. The server shuts down on drop. |
| `scripted` | Data-only response bodies (no axum types in the public surface): `SseEvent { event, data }` (`::data`, `::with_event`), `ScriptedBody::{Json, Sse, Raw}` (`Raw` = byte-controllable malformed SSE), `ScriptedResponse { status, headers, body }` (`::sse`, `::json`, `::text`). Prefer request-matched expectations for inference calls; `enqueue_response(path, response)` remains a compatibility FIFO per path and is still used for non-inference one-shots such as `/v1/settings`. Scripted SSE honors `set_chunk_delay`; matched JSON, raw, SSE, and even empty SSE bodies all honor per-expectation completion barriers. The compatibility `hold_agent_completions` gate also covers foreground scripted SSE on all three inference endpoints. Validation is eager — bad status/header panics at registration. |
| `sse` | The three wire formats as event-list builders: `chat_completion_events` / `responses_api_events` / `messages_api_events(text, model, stop_reason)` (echo-style, whitespace-collapsing) plus byte-exact axum variants `chat_completion_events_exact` / `responses_api_events_exact` and matching public scripted variants `chat_completion_script_exact` / `responses_api_script_exact` (messages is single-delta, byte-exact by construction). The exact/echo split is load-bearing — see the in-module byte-exactness tests. Also the scripted-scenario builders returning `SseEvent`s (for `ScriptedResponse::sse`): `responses_api_reasoning_only_events(reasoning, model)` — reasoning summary deltas completing with a `reasoning` item but no message/output-text, so the shell collector classifies the turn `EmptyReason::ReasoningOnly` (the model-doomloop trigger); `responses_api_reasoning_and_text_events(reasoning, text, model)` — reasoning deltas then a normal text answer (the ordinary reasoning-model turn); `responses_api_reasoning_then_tool_call_events(reasoning, call_id, name, arguments, model)` + its Chat Completions twin `chat_completions_reasoning_then_tool_call_events(...)` — reasoning deltas then one tool call (the think-then-call turn whose tool call finishes the thought and keeps the turn non-empty); the doom-loop check trio: `responses_api_doom_loop_check_events(triggers, reasoning, model)` — a doomed reasoning-only turn with NAMED `response.doom_loop_check` frames re-sent per cumulative prefix of `triggers` plus the terminal `doom_loop_check.triggers` copy on `response.completed`, `responses_api_doom_loop_terminal_only_events(triggers, reasoning, text, model)` — a normal answer whose terminal response alone carries the field, and `responses_api_with_doom_loop_frame(check_frame_data, reasoning, text, model)` — splices one named check frame with a caller-supplied payload (byte-exact `grow_sampling_types::doom_loop::SAMPLE_CHECK_EVENT_DATA{,_CUMULATIVE}` fixtures or malformed variants) into an ordinary turn. |
| `sandbox` | `TestSandbox` — one owner for a temp root, isolated `HOME`/`USERPROFILE`, explicit `GROW_HOME`, workspace, and `TMPDIR`/`TMP`/`TEMP`. Child commands use `env_clear()` plus a minimal platform allowlist, loopback `NO_PROXY`, interactive-git suppression, diagnostics/feedback/trace/instrumentation/updater kill switches, and no ambient leader socket or proxy variables. Unix preserves the host `SHELL` when set and falls back to `/bin/sh`; explicit overrides still win. `TestSandbox::builder().mock_url(url)` wires grow API/models/auxiliary endpoints plus a fake CI key; `.git()` initializes and commits the owned workspace. Bazel test targets that execute Git directly provide `@git_hermetic` runfiles and `GIT_BIN_PATH`; at construction, `TestSandbox` resolves that path against the parent cwd while it is still the Bazel execroot, stores absolute `GIT_BIN_PATH`/`GIT_EXEC_PATH`, and prepends the binary parent to its baseline `PATH`. `TestSandbox::git_command()` applies that cleared environment plus detached, non-interactive Git settings. Without `GIT_BIN_PATH`, ordinary baseline `PATH` is preserved and no special binary/exec vars are added. `set_env`/`extend_env` and `remove_env` are the narrow post-baseline override seam. `diagnostic_summary()` redacts credential-key segments/suffixes and all malformed/non-loopback endpoints; loopback URLs are parsed and stripped of userinfo/query/fragment. |
| `process` | `TestProcess` — canonical Tokio child owner stacked over `TestSandbox`: clears/reapplies the sandbox env, applies `pager_env`, enforces null/piped stdin policy, TTY-detaches, owns the pre-PR3 `xai_tty_utils::ProcessGroup`, and captures bounded stdout/stderr tails. Unix detachment establishes the child session/process group before exec; Windows preserves `CREATE_NO_WINDOW` and uses the existing best-effort post-spawn Job attachment without claiming atomic descendant containment. Private Unix `waitid(WNOWAIT)` observes exit so descendants are cleaned before PID/PGID reuse. `wait_with_deadline` is non-destructive; Unix `close` sends SIGTERM then escalates, while Windows uses immediate Job hard-kill policy; Drop synchronously kills and performs a bounded best-effort reap. PID becomes unavailable after reap; status/reason, truncation counters, read/lifecycle errors, and secret-sanitized tails remain cached. `TestProcessTree` is the process-tree adapter for dependencies that retain their concrete child. All lifecycle policy is test-only; production utility behavior and APIs are unchanged. |
| `acp_client` | `GrowStdioClient` drives `grow agent stdio` over real pipes through `agent-client-protocol`: `spawn` creates a sandbox, `spawn_with_sandbox` reuses one across restarts, and `spawn_with_sandbox_env_and_args` adds explicit env/global-argument overrides. It exposes initialize/authenticate, session create/load, prompt, `*_with_timeout` wrappers, child PID, captured text/stderr, process diagnostics, explicit close/kill signalling, and `take_sandbox`. `RawStdioClient` is the raw-wire sibling for escaped-slash methods and string UUID ids: exact-id response matching skips notifications, auto-refuses agent→client requests with `-32601`, and reports skipped traffic on timeout. Both keep the sandbox alive while `TestProcess` owns the child tree and pipe-tail diagnostics. |
| `headless` | `run_headless[_with_env]` runs grow with an owned canonical `TestSandbox`; `run_headless_in_sandbox[_with_env]` owns a supplied sandbox, while `run_headless_in_sandbox_borrowed[_with_env]` keeps it available for artifact inspection. `_with_env` variants apply explicit last-wins overrides after the hermetic baseline. `TestProcess` owns lifecycle and timeout tree-kill; the scaled 60s process deadline is followed by a separate bounded 2s pipe-drain budget, with retained-pipe or read-task failures returning the bounded partial tail. All variants return `HeadlessResult { status, stdout, stderr, timed_out, elapsed }`. Assertion helpers are `assert_headless_success`, `assert_no_crashes`, and `stderr_tail`. |
| `env` | Binary resolution (`grow_binary()`: `GROW_BINARY` → `CARGO_BIN_EXE` → local debug build) and `git_workdir()`, which returns a git-initialized `TestSandbox`; use `.workspace()` for the cwd. |
| `leader` | Unix-only `LeaderFixture` is mandatory for every `LeaderStdioClient`. It owns exactly one concrete initial leader and the client objects it directly spawns. Callers close/drop clients first; `LeaderFixture::close` rejects active clients and performs bounded TERM→KILL→reap only on the initial owned child/group. If both graceful and hard client cleanup fail, the test-only unwind containment path requests hard kills and intentionally leaks the retained client/leader owners after signaling; this preserves ownership through panic unwind and is bounded by the test-process lifetime. Lock-file PIDs are observations only: detached replacement generations are never adopted or signaled. Death/re-election and version-skew cases that produce detached replacements remain `leader-acceptance` ignored/manual with tracking language until OS containment or a test-only leader binary can own the whole generation chain. No production marker/protocol/bootstrap behavior is required. |
| `uds_proxy` | Unix-only `UdsProxy` — frame-aware (4-byte BE length prefix) man-in-the-middle for leader IPC sockets. `UdsProxy::spawn(proxy_path, upstream_path, FaultPlan)`; `FaultPlan { direction, drop_frame, sever_mid_frame, delay, duplicate_frame }` (1-based frame index, per connection per direction); runtime `FaultHandle::sever_now()` + `forwarded(direction)` counters; frame bodies capped at 64 MiB (leader-transport parity — corrupt lengths error instead of allocating). Zero production changes: point `LeaderClient::connect` / `GROW_LEADER_SOCKET` at the proxy path. |

## Consumer matrix

| Consumer | Uses | Notes |
|----------|------|-------|
| `grow-shell` `tests/*.rs` | `TestSandbox`, `TestProcess` through ACP/leader/headless wrappers, mock server | Binary-driving tests share the same path/env owner; multi-process restart and leader fixtures retain one sandbox across clients. Raw Tokio child ownership is centralized in the wrappers. |
| `grow-pager-pty-harness` | `TestSandbox`, `TestProcessTree`, `MockInferenceServer`, `MockModelEntry` | `ContentController` owns the sandbox and server. `spawn_with_content[_env][_in_dir]` applies that sandbox followed by explicit last-wins overrides. OAuth tests use `EnvOp::Remove` for `GROW_API_KEY`; ordinary overrides use `EnvOp::Set`. `portable-pty` remains the concrete child/wait/signal owner; `TestProcessTree` attaches by PID. Unix gets process-group teardown; Windows attachment is best effort, non-atomic, and reported in diagnostics. PTY exit status is cached so every wait is idempotent; PID/signals disappear after reap; Drop uses a bounded direct-child reap wait. |
| `grow-sampler` `tests/test_actor.rs` | `sse` generators | Happy-path payloads only; the actor keeps its own router for stall/conditional fixtures. |

## Sandbox contract

- Keep the `TestSandbox` alive at least as long as every child using its paths.
- Use the builder only for construction-time endpoint/git choices. Use
  `set_env`/`extend_env` for test-specific flags and terminal brands; the last
  explicit override wins. Use `remove_env` to test absence.
- Do not add process-global env mutation. Keep filesystem/environment ownership
  in `TestSandbox`; process groups, jobs, output tails, and kill-tree ownership
  stay in the separate `TestProcess`/`TestProcessTree` harness.
- Diagnostics may name sandbox paths and sanitized HTTP(S)/WS(S) loopback URLs.
  URL parsing fails closed: userinfo/query/fragment are stripped, while malformed
  or non-loopback values are redacted. Credential-like key segments/suffixes are
  always redacted.

## Adding a capability

**A response mode** (`mock_server.rs`): extend the private `ResponseMode` enum
+ add the setter; wire the new arm into all **three** inference handlers (the
match in each route); scripted responses must still win. Extend the in-crate
tests: an HTTP round-trip for the new mode plus a leg in
`scripted_responses_serve_fifo_per_path_then_fall_back` proving fallback
reaches it. The echo pinning test (`echo_mode_echoes_last_user_message`) must
pass unmodified — echo bytes are frozen.

**A wire format** (`sse.rs`): add the echo-style builder and, if clients
reconstruct text byte-for-byte, an `_exact` variant built on a delta fn;
then add the serving arm in `mock_server` (all modes) and a route if it is a
new endpoint. Extend the byte-exactness pins
(`deltas_reconstruct_multiline_response_byte_for_byte`,
`deltas_preserve_runs_of_whitespace`) — they are the contract that fenced
code blocks (mermaid) survive streaming. A **scripted-scenario builder** (one
that models a specific completion the echo/fixed modes can't express, e.g.
`responses_api_reasoning_only_events`) instead returns `SseEvent`s for
`ScriptedResponse::sse`, needs no `mock_server` mode wiring, and ships with an
in-module shape test asserting its event shape.

**An expectation matcher** (`inference_override.rs`): keep the public matcher
typed and narrow. Claim under the single expectation-state mutex before
serving, replay only overlapping active duplicates by model-call fingerprint,
and add focused tests for auxiliary non-consumption, concurrent one-claim
behavior, lifecycle barriers, and useful unsatisfied diagnostics. Expectations
and compatibility scripts must remain ahead of required auth and fallback modes.

**A scripted-body kind** (`scripted.rs`): new `ScriptedBody` variant + render
arm in `into_response_paced` + eager checks in `validate` if the data can be
invalid. Add an in-crate test asserting client-visible bytes (the `Raw`
byte-exactness test is the template), exercise terminal gating for the new
body, and keep `scripted_response_takes_precedence_over_required_auth` green —
precedence is part of the contract.
