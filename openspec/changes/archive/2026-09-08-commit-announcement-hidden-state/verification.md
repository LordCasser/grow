# Verification

## Implementation
Async wrapper returns io::Result, captures path and serialized snapshot, and executes filesystem operations in spawn_blocking. The private writer creates parents, exclusively creates a same-directory NamedTempFile, writes, syncs and persists it. Failures drop only the owned temp. The sole pager effect maps actual errors into AnnouncementsHiddenPersisted instead of constructing unconditional Ok.

## Coverage
Announcements tests cover canonical roundtrip and replacement, new parent creation, publication against a nonempty directory preserving its contents and an unrelated temp-looking file, blocked-parent failure, and an injected partial-temp-write failure preserving a previously committed regular file with no leaked temp. The write callback is a private fault seam; production supplies write_all.
The corrected pager effect test launches a fresh exact-test subprocess with GROW_HOME set before startup, asserts the resolved path, then tests success and publication against a nonempty directory. It executes the real effect, inspects TaskResult and checks persisted bytes or unchanged obstruction. The first test version was NOT isolated: see test-isolation-incident.md for the confirmed real-state write and unknown prior-preference impact.

## Limits
Publication replaces the destination directory entry (including a final symlink), rather than following a final symlink. No directory fsync/power-loss durability claim. Dropping a waiting async task does not stop an already running spawn_blocking write. Atomic commit does not order concurrent snapshots or merge multiple processes. Reader limits remain unchanged. No Windows or live UI test.

## Results
Announcements library: 11 passed, 0 failed, 0 ignored; 0.04s; process exited 0. Low-disk environment CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216, cargo test --locked --offline -p announcements --lib --quiet. Pager: cargo test --locked --offline -p pager --lib announcements --quiet under the same environment: 45 passed, 0 failed, 0 ignored; 0.04s; process exited 0. The failed first run had 44 pass/1 fail due cached GROW_HOME, then the subprocess fixture fixed isolation. Existing macOS compact-unwind linker warning only. git diff --check passed.
