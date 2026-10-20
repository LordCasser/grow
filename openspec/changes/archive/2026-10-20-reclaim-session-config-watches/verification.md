# Verification — 2026-10-20

## Production ownership and repair

All production watch registrations came from MvpAgent::spawn_and_register_session through the existing local channel, consumed by leader agent/app.rs and Pager acp/spawn.rs. Public unwatch_path had no production caller. Distinct cwd de-duplication therefore kept dynamic paths for the runtime lifetime even after the sessions left.

One RAII registration now lives in existing ResidentResources: this map survives take_session during actor rebuild and drops entries on remove or confirmed idle-unload. Agent destruction drops remaining resources. Guard updates are idempotent by session id at the watcher; repeated updates reuse the same guard and cannot enqueue an obsolete same-id release. Replacing cwd watches the new canonical path before releasing the old key, keeping shared ancestors covered. Last-owner release uses the key captured at acquisition even after deletion/alias retarget. Runtime bootstrap cwd and global/explicit sources remain separate owners; this preserves pre-session/global hot reload rather than treating all watches as disposable session state.

A cancelled/failed rebuild can finish after retiring the old actor but before publishing a new one. Existing SessionLifecycleGuard releases only that nonresident registration when its transaction ends; an in-progress rebuild and any published replacement retain it. There is no await between first watch registration and successful actor insertion. The unavailable leader watcher drops its unused receiver; disabled watching never creates the channel. Existing folder trust and catalog reload policy remain unchanged.

Direct public watch_path/unwatch_path retain their distinct-cwd caller-managed contract; callers must coordinate all references and not mix an independent direct owner with the production session-owned key. Production now exclusively uses session updates. No general external-owner registry is introduced.

## Executed verification

Rust 1.93.1 on native aarch64 macOS, shared default target/debug; CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=4, dev/test debug=0 and split-debuginfo=off. Initial cache fingerprint miss required a dependency rebuild, not a failing test. No separate full target was copied.

- `cargo test --locked -p shell --lib config::watcher:: -- --nocapture`: **17 passed**, 0 failed/ignored. Includes existing real OS late-directory/recreation, shared ancestor and permanent source delivery, plus dynamic session ownership tests.
- `cargo test --locked -p shell --lib config_watch -- --nocapture`: **14 passed**, 0 failed/ignored. Covers repeated same-id registration, two owners of one canonical cwd, replacement cwd, last owner release after directory removal, alias retarget, bootstrap/global/explicit owners, direct production resident-resource teardown, and cancelled rebuild.
- `cargo test --locked -p shell --lib disconnect_ -- --nocapture`: **15 passed**, 0 failed/ignored. Extended existing fake-actor/real-manager unload transactions verify the actor's idle ACK releases a registration, while an actor reporting busy remains resident and retains its watch. No manual state reset or forced Idle substitutes for the unload admission path.
- Cancelled rebuild regression acquires the real lifecycle gate, removes the old handle, parks the future, verifies the registration is retained during the transaction, and cancels the future; dropping the production guard releases the registration. This is a manager lifecycle test, not a full ACP provider session.
- Shared/retarget/source checks feed the same apply_session_update used by both runtime drains, inspect actual watcher registration state and feed late paths through the production callback handler. Existing OS tests retain filesystem delivery coverage.
- Separate read-only adversarial review found no early/double release or old/new actor ownership loss. It identified the caller-coordinated direct API boundary (documented above); unrelated historical rustfmt changes were removed.

`cargo check --locked -p pager --lib`: passed, type-checking both non-test Shell leader and Pager runtime wiring, with no compiler warnings. Final OpenSpec and immutable-source checks are recorded at completion below. No claim of native Windows OS watch validation, real notification quota exhaustion or forced watcher-initialization failure. Failure receiver closure was source-reviewed. Existing child/catalog scope is not expanded; this binds the already registered manager sessions to their owner lifetime. No MCP SDK/protocol change or published release mutation.


## Integrated completion — 2026-10-20

- Full Python discovery with Rust 1.93.1: **29 tests OK, 1 skipped** (the existing OHOS-only smoke case on macOS). This run includes all 16 runner tests and the existing release metadata contracts. Existing Windows storage CI discovers the real cross-platform process-tree fixtures; hosted/native Windows was not run locally.
- Bash production descriptor tests: **29 passed**. Shell watcher tests: **17 passed**; targeted config-watch/owner tests **14 passed**; disconnect tests **15 passed**, all with zero failed/ignored. These scoped sets overlap and are not added as unique test totals. Pager non-test type-check passed. No compiler warnings in these runs.
- All three changes were archived with their deltas merged. Strict current validation: **16 passed**, archived validation: **648 passed**, zero failures. YAML, changed Rust formatting and diff hygiene checks passed; the unrelated pre-existing test formatting was preserved.
- Read-only identity checks confirm the existing v2.4.1 tag object/source and public release with all 11 assets match the preceding immutable receipt. MCP branch, active MCP change, stash, root Cargo.toml/Cargo.lock and Git index remain unchanged. No commit, push or release performed.
- An agent's accidental fresh debug target under target/release-dist was stopped and its own 2.8GB removed. The existing active default target cache was reused for verification; no global/user caches or release-dist artifacts were removed.
