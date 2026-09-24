# Verification

The backlog claim is covered by the existing attempt pipeline, not by an added log or new production path. `AttemptEvidence` captures raw chunks before BOM removal and SSE decoding, retains the observed provider terminal independently of later host rejection, and waits for durable response evidence before candidate use or retry. `run_request_task` settles known/incomplete usage after that evidence and persists the retry or recovery-stop decision before another HTTP request. Shell stores body chunks as immutable BLAKE3-addressed artifacts referenced from the owning Timeline and verifies them on load. Sideband success, failure and retry call `finish_evidence` before their result/next attempt; detached interruption also attempts evidence closure and leaves an open/unknown ledger if a durable frontier was not reached.

New fault injection:

- `incomplete_and_malformed_sse_keep_exact_attempt_evidence`: Chat Completions, Responses and Messages each receive malformed ordinary SSE, an unterminated partial SSE frame, and an empty HTTP 200 body. Every case retains the exact raw bytes, HTTP status and no invented native terminal; the record order is request → response → recovery_stop, with a failed request and no completed candidate or extra HTTP attempt.
- `interrupted_request_preserves_partial_evidence_without_admitting_it`: a durable request and partial response artifact reference are reloaded without a completed request/admission; recovery keeps the evidence, closes the request as `process_interrupted` and the Turn with Recovery authority, and leaves Surface empty.

Existing focused coverage was audited for a real 502 raw invalid-UTF-8 response with blocked/failed/lost response and retry ACK (`response_and_retry_evidence_ack_gate_resampling_and_keep_raw_error`), completed but invalid tool JSON with usage/terminal/decision barriers (`malformed_completed_tool_arguments_recover_with_bounded_accounted_attempts`), Sideband lost ACK (`sideband_retry_waits_for_evidence_and_fails_closed_on_lost_ack`), and successful/failed AI suggestion raw evidence (`ai_suggest_stream_records_terminal_and_raw_body_in_its_sideband_attempt`). Storage integrity is checked by `sampling_evidence::verify` and its reference validation tests; `session_load_preserves_sampling_recovery_stop_evidence` checks cold load.

Commands and results:

- `cargo test --locked --offline -p sampler --lib incomplete_and_malformed_sse_keep_exact_attempt_evidence -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p chat-state --lib interrupted_request_preserves_partial_evidence_without_admitting_it -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p sampler -p chat-state --lib --quiet`: sampler 243 passed; chat-state 523 passed, 2 ignored.
- `cargo test --locked --offline -p shell --lib ai_suggest_stream_records_terminal_and_raw_body_in_its_sideband_attempt --quiet`: 1 passed.
- `cargo test --locked --offline -p shell --lib sampling_evidence --quiet`: 3 passed.
- `cargo test --locked --offline -p shell --lib session_load_preserves_sampling_recovery_stop_evidence --quiet`: 1 passed.
- `cargo test --locked --offline -p shell --lib sideband_retry_waits_for_evidence_and_fails_closed_on_lost_ack --quiet`: 1 passed.

The crash test injects the durable state at the recovery boundary; it is not a power-loss or remote-provider test. Bytes received before a process death but never durably acknowledged cannot be reconstructed, so the open request remains unknown rather than being inferred successful or automatically resent. No production code or behavior contract changed.
