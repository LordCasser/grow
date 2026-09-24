# Verification

- `cargo test -p shell leader::server::tests:: --lib`: 132 passed, 0 failed. The mixed-client test now confirms untagged ACP and Grow notifications reach a non-retracting observer while a candidate is withheld, and a discarded candidate remains hidden.
- `cargo fmt -p shell` and `git diff --check`: passed.
- `openspec validate route-independent-preview-notifications-live --strict --no-interactive`: passed.
- This removes independent notifications from the leader's candidate retention path. It does not prove a hard serialized-byte ceiling for candidate-only notifications; that narrower boundary remains in `openspec/backlog.md`.
