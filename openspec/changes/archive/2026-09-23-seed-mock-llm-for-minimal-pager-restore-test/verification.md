# Verification

- Initial isolated run executed one ignored PTY case but timed out on the “No LLM is configured” startup gate.
- After seeding the mock provider, the test reached its turn but failed to observe the mock response. The already configured `minimal_commits_response_to_scrollback` PTY case passed independently (1 passed), which exposed the additional isolated-project fixture difference.
- With both the mock provider and isolated Git project initialized, `cargo test --locked --offline -p pager --test pty_e2e_minimal minimal_transcript_pager_restore_no_artifacts -- --ignored --test-threads=1` executed one test and passed (9.00 s test time). It reached the real `less` suspend/quit/restore assertions.
- `cargo fmt --all` and `openspec validate --all --strict --no-interactive` passed (16 items before archive).

The case remains ignored by ordinary Cargo runs. The separate default-suite coverage and build-cost backlog entry is still open.
