# Verification (main, 2026-09-08)

Cargo used --locked --offline -p workspace --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0 and CARGO_BUILD_JOBS=2.

- Red regression: 0 passed / 1 failed, 0.01s.
- Final session::file_state::tests: 31 passed, 0 failed, 0.01s.

New coverage exercises invalid JSON syntax and invalid typed-row fields between valid prefix/suffix records, exact physical line 4 after blank lines, failed truncation/merge, no prefix merged, retained live point, metadata fallback, retained lazy source and repaired same-file retry. The repaired file includes whitespace and a valid final record without newline. Existing zero-byte coverage was extended to active pinned full and metadata readers for both empty and blank-only inputs. Existing duplicate-index merge and concurrent live-capture tests continue to pass.

No API or shell call-site changes were needed: InvalidData follows the Result path verified in the preceding change. No new malformed-JSON injection was run through shell actor/pending recovery; tests cover the shared parser and tracker. Metadata scans do not validate all nested FileSnapshot content, and this change does not claim schema equivalence with full materialization. Read budgets, blocking IO and legacy test-only helper removal remain separate.

All fixtures use temporary files. No user history was rewritten, and no installed binary was replaced.

Changed-file git diff --check passed. Strict validation: before archive 17/17, after archive all 16/16, archived 255/255. With no cargo/rustc processes remaining, cargo clean removed 6,179 files / 2.0 GiB. Available disk after clean: 58 GiB.
