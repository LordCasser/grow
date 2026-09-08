# Verification
- On main, locked/offline shell --lib tests with low-disk profile flags (incremental/dev-debug/test-debug off, jobs 2, RUST_MIN_STACK=16777216).
- Red: fallback_titles_fit_canonical_character_limit failed on 500 ASCII characters versus expected 160, with original helper unchanged. This demonstrates a concrete mismatch with Timeline::validate title length, which checks chars().count() > 160.
- Green: session::helpers::session_title::tests passed all 15 tests, 0 failed, 0 ignored, 0.00 s. Includes long ASCII/CJK/emoji, exact 160 characters, cutoff whitespace, existing ten-word selection, skill/Goal handling and empty default.
- Actor source paths confirm provider failure, timeout and invalid output use persist_title_fallback then canonical commit. No real provider failure or complete persistence fault integration was executed; tests cover the changed pure helper and surrounding helper behavior.
- Existing manual-title boundary and Timeline rejection of later generated/fallback titles inspected, unchanged. No user sessions accessed or modified. Linker compact-unwind warning remains unrelated to the helper assertion.
- Strict all 16 / archived 232 passed. cargo clean removed 7,372 files / 2.7 GiB; final available disk 65 GiB.
