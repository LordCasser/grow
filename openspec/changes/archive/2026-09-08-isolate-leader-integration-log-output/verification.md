# Verification
- main. `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --test test_leader_stdio_integration unified_log_redirect_precedes_test_start`: 1 passed, 0 failed/ignored, 48 filtered out; exact child also passed.
- Child GROW_HOME set before startup and asserted against config::grow_home. Emitted marker appears in snapshot; configured home logs/unified.jsonl absent before and after. Constructor uses existing process-temp redirect. No other integration cases or soak executed; no broad Leader behavioral claim.
- R26 is a source-evidence candidate only, no production function deleted. Whole-file snapshot byte budget remains unchanged; current repository consumers are tests.
