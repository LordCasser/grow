# Verification

## Local regression
- Red: actual mock HTTP/SSE session test failed with `serialization error: missing field signature` (0/1, 0.41s). It first established signed native history, then injected a malformed signature delta. No recovery occurred on the old code.
- The initial fixture needed SseEvent wrapping and a move closure. Its 8 MiB test thread overflowed before assertion; raised the test-only helper to 32 MiB, matching adjacent image recovery tests. Production stack settings were not changed.
- Green: sampler Messages group 28/28 (0.02s), shell truncation/recovery group 19/19 (1.69s), exact-field/native-only classifier 1/1 (0.00s): 48 passing checks.
- New tests cover missing initial signature followed by a valid delta, a completed unsigned thought with preserved text/tool facts, strict malformed signature deltas, actual portable retry request bodies without old signatures, exactly one reset on repeated missing signature, and usable later turns.
- Existing API 400 native recovery, failure handling, malformed tool admission and truncation tests remain green. Linker emitted the existing nonfatal __eh_frame size warning.

## Authorized live proxy probe
Read provider.proxy from the user's current Grow config in memory; no credentials were printed or copied into this change. No Claude request was made. Four small HTTP requests were made using Messages protocol:
- gemini-3.8-flash-high: HTTP 500, no usable SSE response; body content was not retained by the first probe.
- gpt-5.6-luna: HTTP 200, 6 SSE events, completed with OK.
- gemini-pro-agent: HTTP 500, upstream TLS handshake timeout.
- grok-4.6: HTTP 200, 22 SSE events, with the previous GPT OK exchange included as history. Thinking start omitted signature; a later signature_delta supplied the field. This matches the previously required-field decoder failure shape.

Replayed both successful raw streams using the newly compiled sampling-types MessageStreamEvent decoder: GPT 6 accepted / 0 failures, Grok 22 accepted / 0 failures. The temporary replay executable required selecting matching serde artifact features; no repository dependency change was made.

Live probes validate provider wire shape and patched decoding, not a complete installed Grow session transition. Full reset/retry behavior is verified by the mock session tests. Gemini live success remains unverified due to upstream failure. Installed binary was not replaced. Raw temporary responses and probe executables are cleaned after recording this summary; signed raw content is not added to the repository.

Scoped rustfmt and git diff --check passed. Pre-archive strict validation passed 17/17. No cargo/rustc process remained before cleanup. Temporary live response directories and probe/replay scripts were removed. cargo clean removed 8,614 files / 3.6 GiB; free disk 56 GiB.
Post-archive strict validation passed: all 16/16, archives 262/262.
