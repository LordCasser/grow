# SDK registration lifecycle verification

## Final parent runtime evidence

固定 Rust 1.93.1 与 locked/offline 的最终 batch `/tmp/grow-262-sdk-leader-final.log` 编译成功，SDK lifecycle **1 passed / 0 failed**，隔离 child 0.55s、外层 0.59s。完整执行 new/resident、SharedMcpPool Arc 继承实际请求/响应、owner 断线不 fallback、原 writer 退役后立即 cold load 并按新连接重绑。两次合法 tools/list 采用准确预期，仍严格校验单次 initialize、单次工具调用、session/注册 ClientId 与内外 ID。

同批 leader target 在准备阶段因已移除的 `--relay-on-demand` 参数失败，使总批次 exit101；SDK 独立 target 已真实通过，未报全批通过。测试支持参数已单独交修，不修改 SDK 生产行为。早期旧 namespace 与序列夹具失败全部保留于原记录，不计产品旧红。

## Fixture and acceptance boundary

`crates/codegen/shell/tests/test_sdk_mcp_registration_owner.rs` adds one process-isolated integration test, `sdk_registration_survives_resident_load_and_rebinds_after_cold_load`. It uses one sandbox and one GROW_HOME for every phase. The parent re-executes the test with HOME/GROW_HOME already installed in the child environment, before any runtime or library initialization; it does not assume a current-thread Tokio runtime makes process environment mutation safe. The subprocess has a 180-second deadline and an owned kill/wait guard.

The fixture starts a real MvpAgent behind its ACP connection and a real leader socket. Its local bridge follows `leader/in_process::spawn_agent`, retaining the MvpAgent Rc to use existing public `get_session_handle`, `SessionCommand::SnapshotInitializedMcpPool`, and SharedMcpPool APIs. No public production hook, SDK update API, or production routing change is added. Two real LeaderClient connections supply accepted connection ids and answer the actual `_grow/mcp/sdk_call` requests. They implement SDK MCP initialize, tools/list, and tools/call replies with the received inner JSON-RPC id, nested inside the matching outer ACP response.

The test is intended to establish these runtime facts:

1. A creates a real Mvp session with an SDK declaration. B invokes its tool through ACP `grow/mcp/call`; SDK initialize/list/call arrive at A with the newly created session id and A's actual accepted owner id, and A's phase-tagged tool response reaches the original caller.
2. B loads that resident session with a valid SDK declaration. The actor command channel and SDK client Arc remain identical. Another actual ACP tool call is answered by A, without another handshake or an SDK request on B.
3. An initialized SharedMcpPool snapshot freezes the current grant and imports the real SDK client into a fresh McpState through `import_shared_clients`. The imported client is the same Arc and the inheritor has no owned clients. Calling through the existing MCP extension function sends a new real SDK tools/call to A, retaining the original registration session and owner, and receives A's corresponding phase-tagged response. This is SharedMcpPool inheritance coverage; it does not claim an actual subagent spawn or broaden the delegated capability matrix.
4. B closes the real session through `grow/session/close`. The test observes removal of residency and closure of the old command channel, then immediately cold loads through the real Mvp lifecycle guard with B's fresh SDK declaration. The cold actor has a new command channel. Its real initialize/list/call exchanges arrive at B with the original session id and B's accepted connection id, and the tool response reaches A as the caller. Close ACK and channel closure alone are not treated as proof of persistence retirement: successful immediate cold admission through the production lifecycle/storage guard is required as well.

The separate A-owner/B-driver, same-serverId, owner-disconnect/no-fallback matrix remains covered by [the SDK producer-to-leader verification](verification-sdk-leader.md). This fixture does not repeat or replace that matrix. Standalone owner=None retains its existing tests in `session/acp_mcp.rs`.

## Commands and current results

The exact command below was queued to the parent (after AC-07 and Skills); this executor did not run Cargo itself. The parent executed it: compilation succeeded and the test reached the real new-session phase, then failed at its first ACP tool call with a `Method not found` error. The fixture had used the obsolete `_x.ai/mcp/call` wire literal, which the canonical extension-method normalization turned into `x.ai/mcp/call`; the current dispatcher serves only `grow/mcp/call` (`extensions/mcp.rs`, verified against the dispatcher). The two `_x.ai/session/close` literals carried the same obsolete namespace. A re-run of the same test reproduced the identical fixture namespace error. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b42b-a03e-7981-828c-93d329398e9b/terminal/call_cd88e3aa91a2467d90777700ec316251.log` (SDK test failed after 2.22 s in that run).

This was a fixture wiring error against the current wire namespace, not evidence against the production SDK registration routing or lifecycle. The fixture has been corrected mechanically: the three literals now use `_grow/mcp/call` and `_grow/session/close` (wire form keeps the leading underscore; the agent dispatcher matches the canonical `grow/...` method names, consistent with the existing `_grow/mcp/sdk_call` wire constant). All lifecycle assertions, real peer/metadata/inner-message checks, and the child writer-retirement chain are unchanged. No production file or public API was changed to accommodate the obsolete literals. The scenarios above describe the test's assertions; lifecycle behavior remains unverified until a green run.

Second parent run (batch log `/tmp/grow-262-shell-final-integration.log`): the batch compiled, the two Install tests passed, and this test again failed in the new phase — this time on the fixture's own sequence assertion: actual SDK exchange `initialize, tools/list, tools/list, tools/call` versus the expected three-step sequence. Because the assertion checks every request's registration session/owner before comparing the sequence, the failure itself confirms both listings carried the correct registration binding.

Both `tools/list` calls are legitimate existing explicit catalog listings of the same SDK client, not a duplicate registration or a re-handshake:

1. The background MCP initialization worker calls `McpClient::get_tool_registrations` per client (`session/actor/mcp.rs`, background handshake loop), which runs `ensure_initialized` (exactly one `initialize` handshake) and then unconditionally `list_all_tools` (`mcp/src/servers.rs`), the init discovery listing.
2. After the handshake the client transitions to Ready and emits `McpClientEvent::Ready`; the session's `SessionRestartActions::refresh_tools` (`session/actor/spawn.rs`) forwards it to `SessionActor::refresh_mcp_server_tools` (`session/actor/mcp.rs`), which calls `get_tool_registrations` again through the same invoker — the Ready-event catalog refresh.

The listing is idempotent: the fixture's SDK peer answers every `tools/list` identically, `initialize` occurs exactly once, and exactly one `tools/call` follows. The correct expectation is therefore the observed two-listing sequence, not deduplication or a loose wildcard. The fixture's new/cold phases now expect exactly `initialize, tools/list, tools/list, tools/call`; the resident and SharedMcpPool-inheritance phases keep expecting only `tools/call`, because both reuse the already-initialized client transport (`ensure_initialized` returns Ready, and inherited-client discovery explicitly reuses the existing transport with no new handshakes), so no listing or handshake can occur there. Registration owner/session/id assertions and the one-call check are unchanged in all phases.

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo +1.93.1 test -p shell --test test_sdk_mcp_registration_owner --locked --offline -- --exact sdk_registration_survives_resident_load_and_rebinds_after_cold_load --nocapture --test-threads=1
```

Use the agreed standard PATH. The command must match one test, and re-execution must also match that same one test. Record the actual command task id, log, exit code, and observed result here after execution; fixture presence or a producer-only test is insufficient to mark the lifecycle task complete.

Checks actually performed:

- `rustup run 1.93.1 rustfmt --edition 2024 --config skip_children=true --check crates/codegen/shell/tests/test_sdk_mcp_registration_owner.rs`: re-run after the wire-literal correction and again after the sequence correction, exit 0 both times.
- `git diff --check -- crates/codegen/shell/tests/test_sdk_mcp_registration_owner.rs`: re-run after both corrections, exit 0; a grep confirmed the only remaining wire literals are `_grow/mcp/sdk_call`, `_grow/mcp/call`, and `_grow/session/close`.
- `openspec validate route-sdk-mcp-to-registration-owner --type change --strict --no-interactive`: exit 0; current change valid.
- `openspec validate --all --strict --no-interactive`: exit 1; 30 items passed, including this change; the unrelated `change/avoid-cross-process-crash-symbols` failed. No files in that change were modified or investigated as part of this task.

## Remaining limits

The lifecycle test compiles but has not yet passed: the first runs stopped at the fixture's obsolete wire literals (corrected), and the second run stopped at the fixture's three-step sequence expectation (now corrected to the verified two-listing legal sequence). New/resident/cold/SharedMcpPool behavior cannot be declared verified until the next run of the same command succeeds against the final fixture; that run belongs to the parent's unified Shell integration window. No production files, other test modules, skills, SDK sources, docs, backlog, or main specs were modified. No archive, commit, push, workspace-wide Rust suite, soak, external SDK deployment, or Windows validation was performed.
