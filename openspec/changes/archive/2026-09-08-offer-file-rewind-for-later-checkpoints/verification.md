# Verification (main, 2026-09-08)

Cargo commands used --locked --offline -p pager --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2 and RUST_MIN_STACK=16777216.

- Red rewind_mode_eligibility_includes_later_checkpoints: 0 passed / 1 failed, 0.06s; selecting target 0 with later changed checkpoint 2 left has_file_changes false.
- app::root::dispatch::tests::rewind: 27 passed, 0.09s. New regression checks unordered later changes, earlier-only changes, inclusive target changes, back navigation and inline-edit exclusion. Existing dispatcher behavior remains covered.
- views::rewind::tests: 18 passed, 0.00s; mode/key/menu behavior remains covered.

Total selected runtime tests: 45 passed. Only the existing linker compact-unwind warning remains. Scoped rustfmt and changed-file git diff --check passed.

The fix changes mode eligibility only; per-prompt counts and target fallback are unchanged. No schema, IPC or backend mutation behavior changed. Tests operate UI state/effects without executing real file rewinds. No interactive installed-app test was performed and no installed binary was replaced.

Metadata read failure still falls back to known in-memory metadata, and schema validation for nested snapshots remains a separate concern. This change does not claim that unknown metadata has become authoritative or that it repairs malformed session data.

Strict validation: pre-archive 17/17, post-archive all 16/16, archived 257/257. After confirming no cargo/rustc remained, cargo clean removed 8,233 files / 3.5 GiB. Available disk afterward: 58 GiB.
