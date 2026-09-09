# Verification

## Baseline

A local channel-backed regression sent a continuous 14,017-character multiline paste (Chinese text plus an unterminated tail). Before the fix, collection emitted only 5,514 characters (7,090 bytes / 394 lines), versus the expected 18,017 bytes / 1,001 lines. The suffix remained queued. This deterministically reproduced the event-budget split, without assuming a particular terminal or clipboard implementation.

Atlas scoped call facts included a stale pre-move `app/event_loop.rs` location; current source and direct call-site reads were used to verify the real `app/root/event_loop.rs` path.

## Cases

- Long code with indentation, blank lines, Chinese comments and an unterminated tail crosses multiple bounded passes. Each pass consumes at most the existing event allowances; the shared collector emits one Paste, the real PromptWidget creates one element, and try_send returns the full payload.
- Exactly exhausting the collection budget with an open sender and no next event flushes on the absolute idle deadline. Subsequent ordinary typing remains a key event.
- Ctrl+C at the next pending-paste batch is retained and does not wait for the remaining queued tail. A control key during initial detection also stops extension without losing the event.
- Existing non-paste event-storm, bounded drain, paste coalescing, bracketed paste and routing tests pass.
- Source review confirms the ACP guard cannot outrank an expired pending-paste deadline. No live provider/session data was changed.

## Commands and results

Environment: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`.

- Pre-fix `cargo test --locked --offline -p pager --lib large_unbracketed_paste_is_one_insertion -- --test-threads=2`: expected failure, truncated paste.
- Targeted event-loop regression: 83 passed, 0 failed before the final detection-boundary case.
- Final `cargo test --locked --offline -p pager --lib -- --test-threads=2`: 7,156 passed, 0 failed, 10 existing ignored tests. Includes all four new cases.
- `git diff --check`: passed.
- Strict OpenSpec validation: 18 passed before archive; 17 current specs/changes and 297 archived changes passed after archive, 0 failed.

## Scope

This fixes the demonstrated unbracketed key-stream batch-cap bug. No interactive test was run in the user's unspecified terminal; arbitrary delivery gaps without bracketed-paste markers remain timing-based. The separate mixed bracketed-paste/key ownership ambiguity is recorded in openspec/backlog.md and was not rewritten in this change.
