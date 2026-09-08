# Verification
- main, explicit temporary filesystem paths, no global home changes or installed binary replacement.
- Before fix, exact client_read_errors_do_not_inherit_shared_grants failed: invalid UTF-8 imported shared shell, Web and MCP grants.
- After fix, `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p workspace --lib permission::state::tests`: 27 passed, 0 failed/ignored, 0.01s.
- Regression verifies full default serialized state, unchanged invalid bytes and directory child, and unchanged shared grants. Existing tests cover NotFound fallback and valid client priority. Directory branch passed after fix; pre-fix regression stops at invalid UTF-8 assertion.
- No FIFO/size/deadline guarantee or change to dangling symlink NotFound semantics; those remain separate audit work.
