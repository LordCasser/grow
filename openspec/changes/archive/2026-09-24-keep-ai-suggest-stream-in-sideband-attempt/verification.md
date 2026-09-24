# Verification

- `cargo check -p shell`: passed.
- `RUST_MIN_STACK=16777216 cargo test -p shell --lib ai_suggest_stream_records_terminal_and_raw_body_in_its_sideband_attempt -- --nocapture`: one real mock-SSE test passed. It covered complete Chat Completions, Responses, and Messages streams, verified a nonempty immutable response body and native terminal for each, and verified a malformed Chat stream retained response evidence and a failed Sideband terminal.
- `RUST_MIN_STACK=16777216 cargo test -p shell --lib recap_display_only_tests -- --test-threads=1 --quiet`: 20 passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed after formatting correction.

The test initially assumed every successful backend observes a transport end. Responses correctly completes at its native terminal without polling to EOF, so the final scenario and assertion require only an actually observed stream-end marker. This preserves the distinction between provider completion and transport completion.
