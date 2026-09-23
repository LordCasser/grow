# Verification

The archived `2026-09-18-analyze-missing-stream-event-type` report identified an Aliyun Messages HTTP 200 stream ending with `event:error` and flat `request_id/code/message`. The current fix carries the SSE name through all three readers and recognizes that exact complete shape only at an `error` boundary. `InvalidParameter` and content inspection fail without retry; `Throttling` uses 429; known overload uses the existing bounded capacity path; an unmapped code retains its identity and vetoes automatic retry. An ordinary frame without `type` still fails protocol decoding.

Validation on 2026-09-23:

- `cargo test --locked -p sampling-types -p sampler -- --test-threads=4`: 573 tests passed across package and integration targets, including real loopback HTTP/SSE for all three backends, the partial Messages tool candidate, strict malformed-field cases, retry classification and bounded 429/overload behavior.
- `cargo test --locked -p shell --lib -- --test-threads=4`: 3,882 passed, 3 ignored. `messages_named_error_rejects_partial_tool_without_retry_or_execution` drove the real turn loop through a partial `tool_use` followed by `event:error`: exactly one provider request, no Timeline tool execution, no promotion of `message_start`'s nonfinal usage, and the later user turn remained usable.
- Changed-file `rustfmt --check`, `git diff --check`, and `openspec validate fix-named-sse-errors --strict --no-interactive` passed. The resolved named SSE debt was removed from `openspec/backlog.md`; its original evidence remains in the archived analysis.

The loopback reproduces the archived wire shape and error boundary; it does not recontact the original external provider or determine why its content policy rejected the text. The existing sampler/session attempt gate remains the owner of output retraction, usage settlement, admission, and shared retry limits; this change adds no alternate scheduler.
