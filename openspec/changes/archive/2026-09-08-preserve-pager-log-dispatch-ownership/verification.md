# Verification

## Scope
Private Forwarder groups the prior globals for isolated tests. The production global and tests use the same initialize, push, take_entries, flush and flush_blocking methods. Public init starts a periodic task only after initialize returns true. No runtime changes outside pager unified logging.

## Cases
A real std::thread asserts absence of a current runtime and pushes 16 numbered entries. The initialized runtime must receive a grow/log notification containing all entries in order. A separate test pushes startup data before initialization, calls both flush variants, and checks retention; after installation, duplicate initialization must return false and delivery must still use the first sender. The fake receiver decodes the production notification and acknowledges it through the real ACP oneshot. Duplicate timer exclusion is supported by the tested installation result plus the public init early-return branch, not a scheduler task-count test.

## Limits
No real global logger initialization, GROW_HOME writes, live application or Windows test. An initialization runtime must remain alive. Existing detached-batch shutdown, unbounded buffer/transport and disconnect retry limitations remain separate debts. Current-batch flush documentation now accurately describes its limited acknowledgement scope.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib unified_log --quiet: 2 passed, 0 failed, 0 ignored; process exited 0. Existing macOS compact-unwind linker warning only. git diff --check passed.
