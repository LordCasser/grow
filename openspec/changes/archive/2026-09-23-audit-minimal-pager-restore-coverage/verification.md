# Verification

- Source audit: `minimal_transcript_opens_in_pager` configures `PAGER=cat`; it cannot cover interactive pager child exit or terminal restore.
- Source audit: `minimal_transcript_pager_restore_no_artifacts` uses `less` in the PTY, sends `q`, and asserts restoration of the minimal idle screen. The test still has `#[ignore]`, so ordinary Cargo runs do not exercise it.
- Host check: `less --version` reported `less 668 (POSIX regular expressions)`.
- Attempted command: `cargo test -p pager --test pty_e2e_minimal minimal_transcript_pager_restore_no_artifacts -- --nocapture`. Cargo compiled for 3m04s and started the target (`running 1 test`), but the parent interrupted it for low disk space before a test result. This is not a passing test and does not establish default coverage or acceptable build cost.
- `git diff --check` passed.
- `openspec validate --all --strict --no-interactive` passed (20 items).

The ignored test remains useful as an opt-in regression, but real external-pager restoration remains uncovered by the default suite. Revisit after identifying a bounded build strategy and recording an actual test result.
