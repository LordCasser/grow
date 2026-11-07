# U1 normal reply completion — verification

Status: implementation and targeted tests complete, pending parent review. All Cargo commands started for U1 have exited. Cargo and write scope are released with the final delivery. This does not accept the whole advisory feature or claim U2/fresh CLI/PTY completion.

## Change

PR winners call the root-owned `completed_advisory_reply_observation` after the existing ViewerFinalized gate and the pre-existing failed-draft early return, before queue drain. Durable winners call the same entry in `AppView::apply_terminal_outcome` inside ViewerFinalized, before drain. The entry captures existing `session.finalized_prompt`, primary session and binding epoch, matches the original card by full identity in that owner's root/child views, and emits the existing QueryInputAdmissionStatus. No public outcome field, terminal registry, card payload, wire or backend was added.

Already canonical Accepted/Rejected cards refuse an Unconfirmed transition and need no new query. Ordinary turns produce no extra query. Losing/late terminal rails retain their existing metadata-only or Ignored behavior and never reach the new hook. The Err + failed-draft path retains its one existing query; U1 adds no second query to it. Normal completion or an optimistic echo is not treated as proof of admission.

Automatic Unknown without a user open intent stays Unconfirmed and does not open an editor; U3's explicit first-open intent still permits the local editor. Existing qualified canonical callback alone establishes Accepted for Allowed/Consumed/Handled or Rejected for Blocked/Dismissed; unknown/error does not resend.

Files changed for U1: `app/root/dispatch/prompt.rs`, `app/root/turn_completion.rs`, `app/root/dispatch/tests/advisory_reply_admission.rs`, and this record. Existing earlier U3 and Settings changes are preserved. Shell/Tools/Goal and U2 retry/parser production behavior were not changed.

## Actual old red

Environment for all commands: `PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`, Rust +1.93.1, locked/offline. Long commands ran in background with timeout 0.

Before U1 production edits:

```sh
cargo +1.93.1 test -p pager --lib --locked --offline u1_
```

Exit **101**: **1 passed / 3 failed**. Noncard completion correctly produced no query; PR winner, durable winner and the canonical-result chain each failed because the expected query was absent. This used actual Submit→Send→TaskResult::PromptResponse Ok with matching meta.promptId, and actual ACP session notification for Grow TurnCompleted (confirmed running foreground), not a direct call to the new helper. Log: `/tmp/grow-ask-reply-completion-oldred.log`.

## Final green

| Command | Exit | Result | Full log |
| --- | --- | --- | --- |
| `cargo +1.93.1 test -p pager --lib --locked --offline u1_` | 0 | 4 passed, 0 failed | `/tmp/grow-ask-reply-completion-green.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline advisory_reply_admission` | 0 | 18 passed, 0 failed | `/tmp/grow-ask-reply-completion-regress.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline turn_completion` | 0 | 24 passed, 0 failed | `/tmp/grow-ask-reply-completion-regress.log` (appended) |

The U1 tests prove:

- A PR winner observes the ended reply ID even though normal drain immediately submits a different next input. A late PR and durable terminal cause no extra query or history growth.
- A durable winner emits one query through app.pending_effects; late PR metadata merge and duplicate durable notification do not query or finish twice.
- Completion query responses Allowed/Consumed/Handled → Accepted, Blocked/Dismissed → Rejected, Unknown/error → Unconfirmed, without editing or resending. A plain successful PR alone never marks Accepted.
- A noncard normal turn causes no admission query.

The full admission suite retains the real PromptResponse error-recovery assertion of exactly one query, U3 editor/first-submit/busy queue tests, identity qualification, immutable text, terminal projections, Goal and stash coverage. The ended-child/cross-tab test additionally completes the actual primary reply and asserts its completion query points at the primary session and the child's original stable reply ID.

Existing turn_completion tests cover first-wins duplicate/stale terminal handling, exact foreground identity, metadata-only late PR, teardown once, recovery and hook/marker behavior. Scoped `git diff --check` and final self-review passed.

## Remaining boundaries

U2's actual Check retry entry and F3 AdmissionReplay distinction remain independently assigned. This change observes a real winning normal terminal; it does not extend the replay parser or reinterpret an admission replay as a new business completion. No CLI rebuild, PTY, full workspace tests, fmt, commit or real user-state/session action ran. Parent still owns product milestone and independent acceptance.
