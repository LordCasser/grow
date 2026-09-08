# Verification

## Source evidence
The old event-loop closure used split_whitespace before Command::new, so quoted executable paths were broken into multiple tokens. Pager already depends on shlex for the editor path. The extracted transcript_pager_command now uses that parser, rejects malformed input or an empty first token, appends the original file as one argument, and returns a direct Command. It does not route the string through a shell.

The existing less option policy is retained verbatim: ANSI snapshots add -R unless one of the prior accepted raw-control flags is present, and +G unless present; other pagers and Markdown receive no added flags. Combined less flags are not newly normalized in this change.

## Coverage
New table tests cover quoted executables, quoted/escaped whitespace, empty arguments, invalid quotes/empty program, less defaults and supplied options, and non-less behavior. A private executable script whose path contains spaces captures NUL-separated actual arguments and checks literal variable/command-substitution/operator text plus the transcript path.

## Limits
The script is a fixed test fixture, not user PAGER configuration. No PTY run or CLI relink is claimed. Existing Unix shell-style argument parsing policy is reused; this is not a Windows-specific command-line parser. Pending request origin is a separate backlog item. No new dependencies were introduced.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib app::root::event_loop::tests --quiet: 79 passed, 0 failed.
- git diff --check passed. OpenSpec all strict before archive: 16 passed.
- Existing macOS compact-unwind-size warning remains. target 13 GiB; disk available 64 GiB.
