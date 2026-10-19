# Verification

## Report and cause

Read only inspection of local session `01a14f53-fdcc-77d2-8dc5-48cde3252075` found three raw `ask_session` calls (`call_6a6a73db5c2d4c40aabe673c`, `call_78206ceef204460fbb20ef65`, `call_6c5d9eb4d5d6468fac83aa72`) with `inquiry_id: ""`. The model's prose claimed omission, but the recorded arguments still contained the blank field. All returned `invalid_request: inquiryId must be a UUIDv7` with an empty result ID. No actual inquiry was sent while investigating.

`AskSessionInput` deserializes missing/null into `None`, but an empty string remains `Some("")`. `SessionCoordinationBackend::ask_session` only generates UUIDv7 for `None`; the runtime then rejects the supplied empty ID before target discovery/admission. Sharing a workspace is unrelated to this rejection.

## Regression and validation boundary

The main working tree's initial Cargo attempt failed in unrelated in-progress planning changes (`tool_types::planning` exports and task input/output fields). Validation therefore uses an isolated source snapshot of HEAD `8cc20454da9e62874170fb3ed1cc532591954947` with only this change's tool implementation, new integration test, and test module declaration overlaid. It does not validate all unrelated uncommitted changes together.

The integration test uses real `AskSessionTool::run`, Serde decoding, `SessionCoordinationBackend`, UUID generation and coordination runtime in a temporary Grow home. Source audit acknowledgements and the target answer are test doubles. No live session, model provider or installed executable is modified.

Before the production fix, the regression failed on the empty-string case with the exact reported UUIDv7 error; omitted and null cases had already succeeded. This establishes that the test detects the reported bug.

After the fix, coverage checks:

- Omitted/null/empty/ASCII whitespace/Unicode whitespace each generate a distinct UUIDv7 and receive an answer.
- An identical retry preserves the existing ID and answer; changing its question returns Conflict.
- Malformed text, UUIDv4 and a UUIDv7 with surrounding spaces remain unchanged and fail with InvalidRequest.
- Runtime ask/get/cancel APIs still reject an explicit empty ID.
- The target receives exactly the five fresh inquiries, with no extra delivery from retries or invalid IDs.

## Results

- Isolated snapshot, before fix: `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test -p shell --lib ask_session_tool_blank_ids --locked --offline` — 1 expected failure with the exact reported empty-ID error. Shared `CARGO_TARGET_DIR` was used for both runs.
- Same snapshot with final source/test overlay: `cargo test -p shell --lib coordination --locked --offline`, with the same environment — **45 passed, 0 failed, 0 ignored**. This includes the new regression and existing independent-process IPC, concurrent retries, conflicting payload, cancellation, approval, recovery and sideband evidence tests.
- The tested tool implementation and new test file were byte-compared with the working tree; both matched. Only the test module declaration's placement differs in the isolated baseline.
- `rustfmt --edition 2024 --check` on the changed tool implementation and new regression — passed.
- `git diff --check` — passed.
- `openspec validate --all --strict --no-interactive` before archive — **21 passed, 0 failed**.
- `openspec archive fix-blank-ask-session-inquiry-id --yes` — archived and merged the checked scenarios into the canonical specification.
- After archive: `openspec validate --all --strict --no-interactive` — **20 passed**; `openspec validate --archived --strict --no-interactive` — **582 passed**, both with 0 failures.

## Adversarial review

The normalization is limited to the model tool entry point. It checks `trim().is_empty()` but forwards every nonblank original string unchanged. It neither repairs malformed retry identities nor changes target IDs, question text, cancellation, source authority, audit acknowledgements, delivery or protocol validation. UUID creation remains in the existing backend. New blank calls intentionally get different identities; explicit identical retries preserve the original identity. No broader coordination redesign or unrelated planning repair was included.
