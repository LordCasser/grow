# Verification

## Red evidence
A real MvpAgent test creates a session at configured max, selects high through the actual model-switch handler, closes it, then resumes in two fresh child processes. Before the fix the second resume's durable fold failed: `invalid model.changed event at seq 10: model change does not continue the preceding durable selection` (0 passed / 1 failed). Each child receives GROW_HOME before process initialization, asserts the resolved home, and writes only its temporary root. The parent asserts each exact child test actually ran.

An earlier same-process close/load fixture encountered active-writer errors (first or second resume). It did not establish the effort failure; changing to independent process lifetimes produced the explicit red evidence above. The writer-release observation is separately recorded in backlog, without adding delays or retries to production.

## Green evidence
Commands use CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216 and cargo test --locked --offline -p shell --lib.

- cold_resume_preserves_durable_effort_chain: 1 passed, 0 failed, 1.89s. Parent runs nine child processes across three cases: high with configured max (including repeat cold load, resident reconnect, actual switch to max and another cold load); persisted None with a subsequently configured max default (cold loads and resident reconnect keep None); unsupported saved high rejected before actor publication. Later processes verify durable transition counts and continuity, so hydration cannot masquerade as an extra model switch.
- session::actor::model_switch::tests: 24 passed, 0 failed, 1.17s.
- model_change: 7 passed, 0 failed, 0.06s, including malformed/discontinuous model-event rejection.

Only the existing macOS linker compact-unwind size warning appeared. No provider requests were needed; the fixture uses an unreachable loopback endpoint. No actual user session was repaired or edited. The unidentified screenshot seq 622 and workflow empty-argument failure remain unproven as the same causal chain; the workflow parse-error path does not directly commit model changes. This change does not repair older corrupt sessions or cover catalog transport identity changes across restarts. No installed grow executable was replaced.

The new test function was formatted in isolation, avoiding unrelated formatting of the dirty shared files. Scoped git diff --check passed. Before archive, all strict validation: 18 passed. After checking no cargo/rustc build remained, cargo clean removed 7372 files / 2.7 GiB.
