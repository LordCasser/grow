# Verification

Date: 2026-10-19. Only source and test changes; no installed executable replacement, user MCP configuration mutation, or real session restart.

## Build boundary

Current workspace compilation is blocked by unrelated in-progress planning changes (`tool_types::planning` / exports and TaskToolInput/SubagentCompletedOutput fields). Logs: `/tmp/grow-mcp-policy-workspace.log` and `/tmp/grow-mcp-policy-green-1.log`. Those changes were not repaired or reverted by this task.

Verification uses an isolated archive of HEAD `8cc20454da9e62874170fb3ed1cc532591954947`, with the previously verified MCP lifecycle source/tests plus the current MCP source and bounded MCP sections of shared session/ACP files overlaid. Unrelated planning/runtime work is excluded. The shared workspace concurrently gained `prevent-uncertain-mcp-tool-replay`; current core MCP source/tests from that work are included in the final overlay because the same call-admission code is shared, but that separate change is not claimed or archived here. A baseline-specific Arc borrow at the subagent helper call is normalized. This validates the recorded MCP overlay, not the whole dirty workspace.

Tests use temporary repositories, isolated GROW_HOME, loopback MCP servers, and an ACP reverse-invoker fixture. Existing negative core tests use documentation-range unreachable addresses; no configured user MCP service or model provider is sampled. Test invocations use `CARGO_TARGET_DIR` pointing at the shared target, `CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`, `RUST_MIN_STACK=16777216`, `--locked --offline`, and one test thread.

## Command and results

```sh
cargo test -p mcp -p shell -p pager --lib --locked --offline -- \
  servers::tests servers::policy_tests acp_transport::tests \
  acp::spawn::tests app::acp_handler::tests::mcp \
  session::mcp_catalog mcp_catalog_lifecycle_tests accepted_client_mcp \
  mcp_server_validation session::mcp_dispatcher session::mcp_restart \
  config::watcher config::reloader extensions::mcp::tests --test-threads=1
```

Current core and Pager result: **169 + 34 passed** in `/tmp/grow-mcp-policy-regression-6.log`. Shell result: **103 passed, 0 failed** in `/tmp/grow-mcp-policy-shell-final.log` after fixture-only isolation repair. Total: **306 related tests passed**. The latter command keeps the same three packages/features and filters only Shell test names; it does not rerun already-passing core/Pager tests.

The first targeted pass had 2 policy core tests, 3 new Pager cases, and 9 selected Shell tests passing. Later broadened runs exposed the intentional old URL-dedup expectation in two existing Pager tests; those scenarios now verify two independent names at one URL, retained original connections, and idempotent reload. They were not removed or skipped. Fixture fixes added a required transport URL for policy-only TOML entries, included an additional shared-workspace validation test module, moved two global-config actor tests into exact-test subprocesses (GROW_HOME is process-cached), and adapted the subagent helper's Arc borrow for the isolated baseline. A failed current-core reset flag assertion was corrected to reflect `reset_transport`'s actual boolean outcome without weakening its no-replay/network-count checks.

Audit checklist results are complete. Targeted rustfmt and changed-file whitespace checks passed. Strict OpenSpec all-validation passed before archive (32 items); archive and post-archive results are recorded below. Temporary sources and task-specific test artifacts are cleaned afterward; unrelated builds and shared libraries are preserved.

## Additional name admission check — 2026-10-19

Pinned Rust 1.93.1, `CARGO_BUILD_JOBS=2`, no incremental/debug info: `cargo test --locked -p mcp --test server_name_validation` passed **1/1** (`validate_mcp_server_names_rejects_duplicates_across_endpoint_and_transport`), log `/tmp/grow-v2-4-0-mcp-names2.log`. This checks duplicate names across endpoints and transports in one client array; it does not replace the Shell update/no-mutation test.

## Coverage interpretation

Policy coverage includes four core policy tests (effective field precedence/reset, an admitted call during shortening and later extension with the ACP backstop, admission before initialization wait, and actual next-handshake startup timeout); two in-process Pager policy tests (watcher-only policy edits and pending initialization/cold load); two isolated actor policy tests (trusted/untrusted cwd, bridge/search/direct-call mask agreement and inherited-client ownership). Existing lifecycle/recovery tests complement the install/stdio-respawn source review; this does not claim a new full stdio process-policy timing fixture.

Identity coverage includes four merge tests, exact duplicate-input rejection before new/load/admin mutation, name/URL mismatch wire counters, authoritative listed URLs, same-name client/disk priority under upsert/toggle/delete, cold load, and existing blocked/late replacement tests. The setup handler's use of the same merge/commit helper was source-reviewed; the mutation integration fixture exercises upsert/toggle/delete rather than a new full setup wizard.

The two actor tests run in exact-test children with GROW_HOME set before process startup; they cannot accidentally read or write the parent process's cached user directory. The core file has only a concurrent explanatory-comment difference from the tested overlay; no unverified policy/runtime change was included after the pass.

OpenSpec: both changes were archived with `--yes`. Post-archive strict all validation and strict archived validation passed (**30 active/spec items and 587 archived changes**).

Cleanup: removed the isolated source archive and pre-change/overlay script copies, plus 390 task-specific test executables/object/dependency files (2363278692 bytes; about 2.20 GiB). Shared libraries, unrelated artifacts, and diagnostic logs were preserved.
