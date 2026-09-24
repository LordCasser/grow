# Verification: stream-projection-digest

- `cargo test --locked -p shell --lib session::response_projection::tests::streaming_projection_digest_matches_serialized_digest_input` with reduced debug/incremental settings: passed (1 test).
- `openspec validate stream-projection-digest --strict --no-interactive`: passed before archive.
- `openspec validate --all --strict --no-interactive`: passed (16 items).
- `openspec archive stream-projection-digest --skip-specs --yes`: completed; no specification changes were merged.
- `git diff --check`: passed before archive.
- `rustfmt --edition 2024 --check crates/codegen/shell/src/session/response_projection.rs`: passed.
- `openspec validate --all --strict --no-interactive`: passed after archive (15 items).
- `openspec validate --archived --strict --no-interactive`: passed (505 archived changes).
