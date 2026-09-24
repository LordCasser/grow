# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p sampling-types --lib cross_segment_duplicate_tool_id_falls_back_to_safe_portable_history -- --quiet`: 1 passed. The test checks Chat Completions, Responses and Messages wire requests: neither duplicate call/result pair survives, while ordinary conversation text remains.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p sampling-types --lib native_tool_id_with_later_neutral_result_keeps_its_native_span -- --quiet`: 1 passed across the three wire backends.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p sampling-types --lib -- --quiet`: 297 passed.
- Targeted `rustfmt --check`, `git diff --check` and strict change validation passed.

The fallback is deliberately conservative: when two distinct calls share a provider correlation ID, it does not choose one tool exchange or alter native bytes. It retains the ordinary portable conversation facts and omits the ambiguous protocol from this request. The Timeline evidence remains unchanged.
