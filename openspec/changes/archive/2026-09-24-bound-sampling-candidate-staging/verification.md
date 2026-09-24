# Verification

- `cargo test --locked -p shell --lib session::persistence::durable_update_tests::`: 18 passed, including fragmented candidate staging and existing projection, discard, and no-projection cases.
- `openspec validate --all --strict --no-interactive`: 16 passed before archive.
- `git diff --check`: passed.

Review confirmed `flush_sampling_candidate` has no accepted-candidate call path: accepted content enters replay through `ResponseReplayProjection`; the staging window needs only the first candidate position and ordered untagged notifications. This change deliberately leaves untagged staging, event channels, and projection copies for the separate end-to-end memory boundary.
