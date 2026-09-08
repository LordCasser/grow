# Verification
- main; no branch switch or commit.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p workspace --lib permission::state::tests`: 26 passed, 0 failed/ignored, 0.01s.
- Explicit temporary directories only; no global home mutation. Actual persistence isolation covers formerly colliding identifiers. Empty/long/Unicode/case/traversal-like identifiers covered by filename mapping checks; shared fallback and priority retain existing IO tests.
- No live ACP UI or installed binary replacement. SHA-256 keys avoid deterministic sanitization collisions; clientIdentifier remains unauthenticated metadata. No legacy migration.
- Strict validation: all 16 passed; archived 213 passed. No active cargo/rustc before cleanup; cargo clean removed 6179 files / 2.0 GiB, filesystem available 69 GiB.
