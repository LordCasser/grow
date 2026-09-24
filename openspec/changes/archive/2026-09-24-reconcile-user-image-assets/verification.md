# Verification

- `cargo test --locked -p shell --lib session::image_describe::tests::`: 23 passed after implementation.
- `cargo test --locked -p shell --lib session::image_describe::tests::user_image_asset_reconciliation_follows_committed_physical_refs`: 1 passed after adding a malformed-Timeline fail-closed case.
- `cargo fmt --all -- --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed before archive.
- `git diff --check`: passed.

The retained-root scan covers physical `Messages`, `Input::Consumed`, and `Notification::Consumed` User carriers. The sweep never infers admission from an API acknowledgement; it reads committed Timeline records while live image publication/commit and cleanup share the input-artifact gate. Startup work is joined at the session's final storage frontier. A failed live commit performs the same reconciliation after releasing its publication gate.
