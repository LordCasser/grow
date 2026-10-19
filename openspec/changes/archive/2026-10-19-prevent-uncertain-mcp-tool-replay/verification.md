# Verification

The test changes exercise the single-dispatch contract and retain the relevant connection lifecycle cases. Recorded compiled-library test evidence follows.

| Contract | Test evidence | Status |
| --- | --- | --- |
| Transport response loss does not replay a possibly completed operation | `try_call_tool_does_not_replay_after_server_effect_and_lost_reply` records one server-side effect before dropping the real MCP transport, checks the unknown-outcome error and one effect, then proves a separate explicit invocation can reconnect. | Passed in the MCP library run below |
| HTTP MCP errors do not replay the original request | `try_call_tool_http_mcperror_resets_for_next_call_without_replay`, `try_call_tool_http_mcp_error_surfaces_without_second_call`, and `try_call_tool_http_mcp_error_is_not_reclassified_as_retry_timeout` cover single dispatch and, where eligible, reset for a later invocation. | Passed in the MCP library run below |
| Genuine timeout remains unknown, resets for later explicit use, and is not replayed | `try_call_tool_http_outer_timeout_resets_transport_no_retry`. | Passed in the MCP library run below |
| A late error cannot reset a replacement transport or retry on it | `late_tool_error_does_not_reset_already_recovered_service`, `retired_client_late_tool_error_does_not_recover_or_retry`. | Passed in the MCP library run below |
| Exact peer cancellation and dropped-call handling | `timed_out_mcp_tool_call_cancels_its_exact_request_once`, `dropped_mcp_tool_call_cancels_its_exact_request`, `abandoned_mcp_call_never_cancels_replacement_service`. | Passed in the MCP library run below |

## Executed — 2026-10-19

Executed the compiled MCP library test binary `target/debug/deps/mcp-e13a81b5b42e7c19 --test-threads=2` with `RUST_MIN_STACK=16777216`: **178 passed, 0 failed**, 8.00s. Log: `/tmp/grow-v2-4-0-mcp-lib-tests3.log`. Its test inventory includes the server-effect/lost-response case and the exact tests listed above, which all passed.

The production change, deadline-policy change, and their tests were already present in this binary. A later comment-only correction does not alter this result. No real external MCP service or MRTR interoperability was tested or claimed.

## Final validation — 2026-10-19

Pinned Rust/Rustdoc 1.93.1, two build jobs, 16 MiB test stack and no incremental/dev/test debug info: `cargo test --locked -p mcp --lib -- --test-threads=2` passed **178 tests, 0 failed**, 7.99s; `/tmp/grow-v2-4-0-mcp-final51.log`. The server-effect/lost-reply, HTTP errors, genuine timeout, exact peer cancellation and replacement tests above all pass again on the current source. `RUSTUP_TOOLCHAIN=1.93.1 cargo fmt -p mcp -- --check` passed (`/tmp/grow-v2-4-0-mcp-fmt48.log`); the separate attempt to invoke an old removed binary in `mcp-final48.log` did not run tests and is not used as evidence. Developer notes and the 2.4.0 changelog describe the single-dispatch behavior. Strict all-change validation is recorded in the archive command log.
