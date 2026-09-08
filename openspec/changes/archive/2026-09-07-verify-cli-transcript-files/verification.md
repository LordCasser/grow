# Verification

- Branch: main. Built the current mixed working tree without resetting unrelated changes.
- Build: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo build --locked --offline -p cli --bin grow --quiet, exit 0.
- Artifact metadata and three successful read-only smoke checks are in artifact.json. Each command returned nonempty stdout and empty stderr, exit 0, within a 20-second timeout.
- Includes archived background-transcript-file-writes and fix-private-pager-transcripts. Their earlier targeted tests passed (38 and 11 respectively); this build check does not repeat those tests or claim real PTY validation.
- Existing macOS compact-unwind-size linker warning remains. No installed binary was replaced. Version Git hash does not identify mixed-tree content; the recorded SHA-256 identifies this artifact.
- target 13 GiB; available disk 64 GiB. Retained useful current build cache; no compiler was interrupted and no cargo clean ran during compilation.
- Source audit found ignored external pager process errors in app/root/event_loop.rs; recorded separately in backlog, with no behavior changes in this verification change.
