# Verification
- main, locked/offline shell --lib cleanup_ttl, incremental/dev-debug/test-debug off, jobs 2 and RUST_MIN_STACK=16777216.
- 3 passed, 0 failed, 0 ignored, 0.00 s. Parser cases cover missing storage/TTL, 90 days, exact u32::MAX, 2^32, i64::MAX, zero, negative, float, boolean, string and malformed storage section.
- Adapter test invokes invalid TTL 0/u32::MAX against a deliberately non-directory sessions sentinel, asserts InvalidInput before scanning and unchanged bytes, then confirms ordinary 30-day cleanup on an empty temporary root succeeds with zero removals. No real home cleanup invoked.
- ConfigLayers missing-file versus syntax/read-error behavior inspected: missing becomes an empty table, actual errors propagate. No global-home resolver fault injection was run; pure parser and adapter boundaries are directly tested.
- No prior-production panic/deletion reproduction, nonempty-history deletion test or activity-race fix claimed. Remaining recheck-session-cleanup-activity-under-lease is independent and active.
- Existing large __eh_frame compact-unwind linker warning; selected tests passed.
- Strict all 17 / archive 240 passed. Confirmed other active Cargo build targets ScriptOS and left it alone. Grow cargo clean removed 7,372 files / 2.7 GiB; disk available 64 GiB.
