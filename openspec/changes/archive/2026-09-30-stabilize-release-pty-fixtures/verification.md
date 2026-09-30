# Verification

- The focused `minimal_new_session_keeps_history_and_resets` PTY case passed after the independent `minimal-welcome-after-session-binding` runtime fix.
- All 20 selected release PTY cases in `.github/workflows/core-regression.yml` passed with the rebuilt 2.3.0 CLI and the CI test-profile environment. The individual command outputs are `/tmp/grow-pty-*.log`.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive` passed before archive.
