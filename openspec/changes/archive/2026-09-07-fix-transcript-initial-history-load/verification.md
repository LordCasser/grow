# Verification

## Reproduction and implementation
The new three-mode regression failed against the old implementation in the first Inline case: pending_pager_path was set while loading_replay was true. This is the observed red test; Fullscreen/Minimal old failures were not separately executed after that first assertion.

The regular dispatcher now guards the actual with_active_agent target before collecting/rendering blocks, and returns without adding a misleading empty-transcript notice. Minimal guards its actual root-agent source before collecting IDs, except when SessionReload is active, which retains the already-specified waiting/restart behavior. SessionLoaded clears loading_replay; SessionLoadFailed clears it and unbinds the failed session as before.

## Test scope
The regression iterates Inline, Fullscreen and Minimal: no file/build during partial history; loading notice present; after clearing the flag and appending the final message, Markdown contains both parts or the minimal build includes all current IDs. Existing transcript dispatch tests also cover minimal reconnect waiting/restart, explicit file jobs, selection, export paths, and owned temporary files.

## Limits
The test drives actual dispatch and reads actual temporary Markdown files; it does not drive a network replay or launch an interactive pager. Minimal checks the exact ID snapshot passed to its unchanged pump, not a new ANSI output assertion. No CLI relink is claimed.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib app::root::dispatch::tests::transcript --quiet: 22 passed, 0 failed.
- git diff --check passed. OpenSpec all strict before archive: 16 passed.
- Existing macOS compact-unwind-size warning remains. target 13 GiB, disk available 63 GiB.
