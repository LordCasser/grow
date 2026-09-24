# Verification

- `cargo check -p shell --tests`: passed.
- `cargo test -p shell response_projection --lib`: 23 passed, including pointer sharing, serialization round-trip, raw/typed reconciliation, cold repair, and fork expansion.
- `cargo test -p shell projection --lib`: 98 passed, including durable exact-match, resident delta, replay cursor, and projection failure gates.
- `cargo fmt -p shell`: passed.
- `openspec validate --all --strict --no-interactive`: 16 items passed before archive, 15 after.
- `openspec validate --all --strict --no-interactive --archived`: 512 archived items passed.
- `git diff --check`: passed.
