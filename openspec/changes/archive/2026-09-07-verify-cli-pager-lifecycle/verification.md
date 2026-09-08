# Verification

- Branch main; current working-tree sources retained without reset or unrelated edits.
- Build: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo build --locked --offline -p cli --bin grow --quiet. Exit 0.
- Includes fix-pager-process-feedback, fix-pager-quoted-arguments, and fix-transcript-reload-restart. Previously recorded tests remain their behavior evidence; this change verifies the CLI composition/build.
- --version, --help, export --help: each exits 0, nonempty stdout, empty stderr, under a 20-second timeout. Actual artifact size, SHA256, version and output byte counts are in artifact.json.
- Existing macOS compact-unwind-size linker warning remains. No real pager, reconnect, PTY, or installed-binary replacement is claimed. Version Git hash alone does not identify mixed working-tree content; use the artifact SHA256.
- target 13 GiB; disk available 64 GiB. No cleanup during compilation; retained useful current cache.
- OpenSpec all strict validation before archive: 16 passed.
