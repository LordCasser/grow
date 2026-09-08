# Verification

- main working-tree CLI built successfully with CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo build --locked --offline -p cli --bin grow --quiet (exit 0).
- Includes fix-transcript-initial-history-load, fix-input-dump-file-creation, fix-scroll-log-default-collision, and fix-scroll-log-failure-state; their archived targeted tests remain the behavior evidence. The audit-only recorder comment correction is included too.
- --version, --help, export --help each return exit 0, nonempty stdout and empty stderr within 20 seconds. Artifact hash, size and exact byte counts are recorded in artifact.json.
- Version Git hash alone does not identify the mixed working tree; SHA256 identifies the actual built file. No installed version was replaced. No PTY/reconnect/gesture end-to-end test is claimed in this build check.
- Existing macOS compact-unwind-size warning remains. target 13 GiB, disk available 64 GiB. No cleanup during compilation; retained useful current cache.
- R13 raw key formatter and self-test still exist, awaiting user deletion confirmation.
- OpenSpec all strict before archive: 16 passed.
