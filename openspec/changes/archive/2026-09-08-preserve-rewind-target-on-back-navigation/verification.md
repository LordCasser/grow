# Verification (main, 2026-09-08)

Cargo used --locked --offline -p pager --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2 and RUST_MIN_STACK=16777216.

- Red rewind_back_from_confirmation_preserves_older_target: 0 passed / 1 failed, 0.06s; selected target 2 became 4 after the actual preview/confirmation/back dispatcher sequence.
- Final app::root::dispatch::tests::rewind: 28 passed, 0 failed, 0.09s. The new test traverses actual RewindSelectMode and RewindPreviewComplete actions, confirms the expected confirmation phase, then returns. It checks target 2 with newer checkpoint 4, preserves target 2 when the cached point list is removed, and retains target 0 from ConversationOnlyConfirm. The resolved selection also equals the original target.

Existing range-based file eligibility, inline-edit back behavior and other rewind dispatch tests remain passing. Only the existing compact-unwind linker warning remains. Scoped rustfmt and changed-file git diff --check were checked before archive.

No view input mapping, IPC schema, backend rewind execution or file restoration changed. Tests manipulate UI state/effects; no real rewind effects or installed-app interaction were executed. No user session data or installed binary was changed.

Strict validation: pre-archive 17/17, post-archive all 16/16, archived 258/258. After confirming no cargo/rustc processes remained, cargo clean removed 8,233 files / 3.5 GiB. Available disk afterward: 58 GiB.
