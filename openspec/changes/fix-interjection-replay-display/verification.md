# Verification

- Baseline: `interjection_replay_preserves_user_text_and_model_envelope` failed because persisted display included the runtime prefix and nested user_query wrapper instead of the original input. Log: `/tmp/grow-interjection-baseline.log`.
- Fixed regression passes for both direct injection and durable accepted steering. Captured persisted ACP notification is serialized/deserialized as replay data; user-authored markup remains exact and model ConversationItem still contains the interjection envelope.
- No user session files or existing historical display rows are rewritten. This corrects display records produced by the fixed build; model Timeline, input identity, image blocks and permission evidence retain their existing path.
- Validation environment: macOS, `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`.

- Full Shell library regression: `cargo test --locked --offline -p shell --lib -- --test-threads=2`: **3,761 passed, zero failed, 3 ignored**. Log: `/tmp/grow-interjection-fixed.log`.
- `git diff --check` and `openspec validate --all --strict --no-interactive` passed.
