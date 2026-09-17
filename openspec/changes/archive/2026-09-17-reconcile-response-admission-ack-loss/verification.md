# Verification

## Implemented checks

- `cargo check --locked -p chat-state -p shell` — passed.
- `cargo test --locked -p chat-state --lib` — passed (489 passed, 1 ignored).
- `cargo test --locked -p chat-state --lib 'response_admission' -- --nocapture` — passed (4 passed).
- `cargo test --locked -p shell --lib response_admission_errors_use_the_fatal_boundary_marker` — passed (both `AcknowledgementLost` and `ResponseAdmissionConflict`).
- `cargo test --locked -p shell --lib sampling_candidate_is_scoped_and_dropped_until_durable_admission` — passed; existing preview stays unaccepted until the durable gate and is discarded on failure.
- `rustfmt --edition 2024 --check <changed Rust files>` — passed.
- `git diff --check` — passed.

## Coverage and limitation

The Timeline tests cover completed request/attempt matching, wrong attempt, illegal metadata shape, wrong quarantine result, legacy metadata absence, bulk replay, and duplicate raw identity rejection. New ChatState actor tests use `TestHarness::with_manual_timeline_ack_after` to seed a live Turn/Step/Request lifecycle, drop the caller before raw ACK, then exact-reconcile; they cover one raw response, conflict immutability, and malformed raw/repair ordering with exact quarantine result. The Shell evidence is compositional: the real identity-bearing production call maps admission errors to the typed `response-admission` fatal wrapper, while the actor tests prove durable commit/reply-loss and exact reissue. The current harness does not provide owner replacement or end-to-end provider-count/Accepted/tool injection, so no such claim is made; current-turn uncertainty remains fail-closed and no same-handle retry is performed.

No whole-workspace test was run. After all Rust checks completed, `target/` was 12 GiB and the filesystem had 112 GiB free. `cargo clean` removed 57,101 files / 12.5 GiB; afterwards `target/` was absent and free space was 119 GiB.

## Independent review

A read-only independent review first identified and blocked the ineffective same-dead-handle retry plus missing deterministic fault evidence. After removing that retry, adding actor commit→caller-loss/conflict/repair tests, and routing the real Shell branch through the tested fatal mapping helper, the reviewer reported no blocking findings. The documented residual limitation remains the absence of one owner-replacement end-to-end harness that simultaneously counts provider calls, Accepted updates and tool dispatch; this change does not claim that coverage.

## OpenSpec

- `openspec validate reconcile-response-admission-ack-loss --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed (19 active specs/changes).
