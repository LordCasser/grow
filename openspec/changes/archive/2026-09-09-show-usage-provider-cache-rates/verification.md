# Verification

2026-09-09, main, macOS arm64.

- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test -p pager -p shell --lib`: **10,930 passed, 0 failed, 13 ignored**. Pager 7,160 passed / 10 ignored; shell 3,770 passed / 3 ignored. The existing large unwind-table linker warning did not prevent execution.
- `openspec validate --all --strict --no-interactive`: 18 passed before archive.
- `git diff --check`: passed.

Scenario evidence:
- `usage_keeps_selected_provider_models_separate_from_wire_aliases`: records A/shared, B/shared, A/shared with identical response wire aliases. The ledger retains exactly two provider/model entries, 300 and 150 total tokens, and 450 overall. The production turn captures its catalog identity before awaiting the sampler and passes that identity to the same recording boundary.
- `cache_rates_use_input_weighting_and_keep_model_identity`: 100 input/100 cached plus 900 input/0 cached yields 10.00% overall, with separate 100.00% and 0.00% model rates. A single model still shows its identity. Inconsistent derived wire total is not used in place of input+output.
- `cache_rates_handle_zero_invalid_and_large_counts`: zero denominators and cache > input render N/A; zero cache, large u64 counts and fractional rounding are covered. Output-only entries render N/A overall and per model.
- Existing empty/incomplete usage and unknown-cost tests pass; updated snapshots pin cache rates and layout. Incomplete percentages explicitly cover recorded usage only.
- Modal and minimal mode both call `session_usage_block_text`; Session Info code and wire shape were not changed.

Counting window remains since session start or last resume, as labeled. Historical ledger reconstruction is recorded separately in backlog; no session logs, provider configuration, or live provider calls were changed.

Post-archive: 17 active specs/changes and 300 archived changes passed, zero failures. After confirming no Cargo/rustc process remained, `cargo clean` removed 8,252 files (3.9 GiB).
