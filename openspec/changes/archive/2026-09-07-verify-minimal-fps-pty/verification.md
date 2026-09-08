# Verification

## Setup
Built main working-tree CLI using CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo build --locked --offline -p cli --bin grow --quiet, exit 0. Actual artifact hash/size and successful --version output are recorded in artifact.json. Existing macOS compact-unwind-size linker warning remains.

The new ignored PTY regression belongs to the existing pty_e2e_minimal family. It uses ContentController's isolated HOME/GROW_HOME and local mock service, sets GROW_FPS=1 to verify startup activation, enables terminal query replies to ensure actual minimal mode, and runs only the new case with one test thread. It does not call a user model or modify the installed binary.

## Intended assertions
Observe startup HUD, turn it off, then open /debug fps again and observe HUD; wait for cached-stats cadence and type a draft to trigger a normal frame; assert numeric stats, at least two rows separating HUD and prompt, and cursor on draft. Clear draft, turn off HUD, assert no stale title cells, type another draft and verify cursor. Quit using existing minimal helper.

## Execution
Passed: 1 test, 0 failures, 28 filtered out, 5.25 seconds. Command: `PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --test pty_e2e_minimal minimal_fps_hud_toggle_preserves_prompt -- --ignored --test-threads=1`.

Fixture failures were investigated before the pass: an unseeded model config showed the BYOK setup; after seeding, the sandbox HOME was a non-project directory, so the project picker consumed the command keystrokes. A pre-submit screen assertion exposed that question. The final fixture seeds the mock provider and initializes only its temporary HOME as a Git project. No production changes were needed for these failures.

Before cleanup, target occupied approximately 15 GiB and the filesystem had 62 GiB available. No cargo/rustc/linker processes were running at the cleanup boundary. Artifact metadata records the tested build; cargo clean removes that build from target afterward.

Cleanup completed successfully: `cargo clean` removed 21,282 files, 15.0 GiB total.
