# Verification

## Implementation and evidence
Input recording now wraps actual AgentView input routing. Delegated keys record only at the child that handles them; parent interception records locally. Minimal btw early returns also record locally, and root no longer duplicates observations. Each key starts with a fresh textarea delta. A diagnostics-specific resolver follows child ownership with parent permission priority and supports dashboard attached sessions. The dump builder reads the selected view without file IO.

## Results
Low-disk environment: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216.
- cargo test --locked --offline -p pager --lib input_diagnostic --quiet: 3 passed, 0 failed, 0 ignored (0.04s).
- cargo test --locked --offline -p pager --lib input --quiet: 268 passed, 0 failed, 0 ignored (0.29s), process exited 0. Includes input router, minimal input and input_log regression tests.
- git diff --check passed.
Existing macOS compact-unwind linker warning only.

## Scenario coverage and limits
New tests enter through AppView input routing with distinct parent/child session and textarea state, check one child observation and no parent duplicate, printable key redaction, child dump metadata, attached dashboard target resolution, no attached target, empty ring, and parent Esc closing a bare child without stale delta or child attribution. Parent permission priority is verified by code path inspection; existing input tests exercise permission/minimal behavior, but no new permission-specific diagnostic assertion was added. Existing input_log tests cover bounded ring and private unique file persistence.

Dashboard Esc closes the popup before child input: this is target-resolution coverage, not an end-to-end popup dump shortcut test. No real GROW_HOME/log access, installed CLI replacement, live terminal or Windows validation. No claim of a new user-facing shortcut.
