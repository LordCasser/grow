# Verification

## Incident and baseline

Read-only inspection of session `01a081be-6168-7772-9e0c-dcc62a76b552` found request `364a7e95-4317-4e2e-8b90-e35ab3613c6b` returned HTTP 200 with complete, non-truncated evidence. The function arguments at output index 2 were identical in arguments.done, item.done and response.completed and ended with `"description":: `. This is malformed upstream generation, not lost Grow stream fragments. The invalid call never appeared as an executed tool in the Timeline.

The local mock HTTP/SSE regression reproduced a completed malformed function call stopping after one request, as non-retryable Serialization with known 14-token usage. It failed the expected recovery request-count assertion before the implementation. An initial fixture omission of Responses usage-detail fields was corrected before recording this baseline.

## Boundaries

- Whole-response validation rejects valid siblings with the invalid call. Any unfinished sibling remains a non-retryable protocol error regardless of item order.
- The typed error survives the serialized SamplingErrorInfo boundary; ordinary Serialization stays non-retryable.
- The mock server compares retry input against the original valid input. A failed attempt never emits Completed, so no tool dispatch/native-continuation acceptance occurs.
- Known usage and retry evidence are acknowledged before retry. Failure of either sink or cancellation prevents another request.
- Recovery emits existing Retrying notifications. Source inspection confirmed Shell projects these without a turn failure; Pager intentionally keeps normal running activity instead of adding a warning.
- Existing strict Responses identity/status checks are retained. No real provider credentials or live session files were modified. No new binary was installed or released.

## Regression results

`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p sampler -p sampling-types -p shell --lib -- --test-threads=2`

- sampler: 220 passed, 0 failed.
- sampling-types: 267 passed, 0 failed.
- shell: 3,762 passed, 0 failed, 3 ignored by existing configuration.
- Total: 4,249 passed, 0 failed, 3 ignored.
- `openspec validate --all --strict --no-interactive`: 18 passed, 0 failed before archive.
- `git diff --check`: passed.

Final `cargo test --locked --offline -p sampler --tests -- --test-threads=2` with the same environment: 220 unit tests and 33 integration tests passed, 0 failed. This includes the added unknown-usage scenario (Incomplete settlement, then successful resampling), plus existing fatal generic serialization and transport-after-output fences.

Unique affected coverage: 4,282 passed, 0 failed, 3 ignored. No live provider run was needed: the mock uses the incident's malformed-argument shape deterministically. Post-archive validation passed: 17 current specs/changes and 296 archived changes, 0 failed.

