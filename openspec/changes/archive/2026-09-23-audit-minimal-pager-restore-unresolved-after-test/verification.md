# Verification

- Host check: `less --version` reports `less 668 (POSIX regular expressions)`.
- The test source checks for `less`, sets `PAGER=less`, submits a committed turn, opens `/transcript`, waits for the `grow-transcript-` child UI, sends `q`, and asserts restoration of the idle screen. The `#[ignore]` attribute remains, and the test was not enabled by this audit.
- A short-name `--exact` invocation ran 0 tests (30 filtered); this was discarded as a false-positive attempt.
- The exact libtest case name is `minimal::minimal_transcript_pager_restore_no_artifacts::minimal_transcript_pager_restore_no_artifacts`. Runs with that exact filter did execute one test. Without provider config, the first run stopped at the first-run “No LLM is configured” prompt. After seeding the mock provider, subsequent runs reached `minimal · /help` but timed out waiting 30 seconds for `MOCKRESPONSE`; the diagnostic showed `mock requests: []`. A provider credential was then added to the test helper under `[provider.mock.options]` and the test was rerun; it produced the same no-request timeout. In all actual runs the test failed before opening `/transcript`; no `less` child or restore assertion ran.
- The attempted command was `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test -p pager --test pty_e2e_minimal 'minimal::minimal_transcript_pager_restore_no_artifacts::minimal_transcript_pager_restore_no_artifacts' -- --ignored --exact --nocapture`. The final run returned 101 after one failed test; free disk remained 65 GiB. The temporary test/helper diagnostics and fixture edits were reverted afterward.
- `git diff --check` and strict active/archived OpenSpec validation are run as part of closing this audit.

Default external-pager restore coverage remains unresolved. Keep the backlog item until a bounded run reaches `less`, sends `q`, and passes the restore assertions.
