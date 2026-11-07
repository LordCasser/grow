# U2 explicit frozen reply retry — implementation and verification

Status: bounded U2 implementation and targeted verification complete, pending parent review. All task commands have exited; Cargo and write ownership are released with delivery. No fresh CLI/provider/PTY, Windows, or whole-feature acceptance is claimed.

## Implemented behavior and ownership

The actual QueryAdvisoryReplyAdmission action's existing one-shot open intent now distinguishes an explicit Check/retry from automatic reconciliation. After the existing owner/session/epoch/prompt/durable and monotonic-card qualification, matching UnknownUnconfirmed or Absent can release an existing frozen queue/in-flight snapshot only while the session is idle and no current foreground/RPC owns it. The original complete body and stable prompt ID enter the existing ordinary FIFO once. No identity parsing, replacement body, card payload copy, state field, registry or wire was added. Unknown remains an uncertain observation; the existing F3 atomic identity/payload owner provides new-admission/replay/conflict semantics.

Automatic observations never gain the explicit retry intent. Submitted, timeout, unavailable, invalid evidence and transport/decode errors do not permit retry. Active submissions cannot send twice; quick second clicks coalesce and late observations retain the original immutable body. Existing ready first-input rows retain U3's normal FIFO qualification. Accepted/Rejected remain canonical and read-only.

A private PR-consumption helper recognizes grow/inputAdmission metadata for an exact locally retained advisory identity, validates session/prompt envelope and a nonempty input ID plus supported typed status, and consumes the observation separately from the ordinary business finalizer. Only that exact TurnSubmitting placeholder can be retired to Idle; the immutable snapshot stays available and a valid observation emits the existing canonical query. Replay status itself never marks a card accepted. Invalid metadata retains an unconfirmed snapshot without claiming success. Duplicate replay responses and a different running foreground do not finalize, bill, notify, add a TurnCompleted marker, or clear that foreground. Ordinary PR and U1 winning terminal paths remain unchanged.

When canonical settlement removes the retry barrier, the existing maybe_drain_queue guards may advance the next ordinary input. This handles replay retaining only an in-flight snapshot as well as an existing review row; it cannot resend the terminal reply or override an active foreground/editing gate.

U2 changed `app/root/dispatch/prompt.rs`, the existing `dispatch/tests/advisory_reply_admission.rs`, and one existing Unconfirmed hint string in `scrollback/blocks/tool/other.rs` to say check or retry the original answer. No task_result observer qualification, actions/router/mouse, Shell, Tools, Goal, or public protocol source was changed. Other uncommitted work predates this task and is preserved.

## Actual old-red evidence

All Cargo invocations used PATH="$HOME/.cargo/bin:$PATH", RUST_MIN_STACK=16777216, CARGO_BUILD_JOBS=2, Rust +1.93.1, --locked --offline, background execution and timeout 0.

`cargo +1.93.1 test -p pager --lib --locked --offline u2_` before production edits exited **101**, **1 passed / 2 failed**. The real first Submit → Send → PromptResponse Err → automatic observation → actual Check → Unknown/Absent chain failed to retry, and the actual admission-replay PR consumer did not return the required canonical observation. Log: `/tmp/grow-ask-reply-retry-oldred.log`.

Parent review identified that the subsequent ordinary row could remain stalled after the retry barrier settled. An actual Err → review → normal input queued behind → Check/retry → AdmissionReplay → canonical Handled/Blocked regression was added before correcting that wakeup; it exited **101**, **0 passed / 1 failed**. Log: `/tmp/grow-ask-reply-retry-queue-oldred.log`. A first attempted wakeup conditioned on a pre-existing queue row remained insufficient because replay can retain only in-flight; the final change attempts the guarded ordinary drain on canonical settlement.

An intermediate full suite had one old U3 expectation fail: explicit Check with frozen text had previously expected no send. It was adapted to U2's now-authorized same-body retry while preserving U3's assertion that no selection editor opens. No immutable body, source, terminal, Goal or stash assertion was removed.

## Final verification

| Command | Exit | Result | Full log |
| --- | --- | --- | --- |
| `cargo +1.93.1 test -p pager --lib --locked --offline u2_` | 0 | 6 passed, 0 failed | `/tmp/grow-ask-reply-retry-green.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline advisory_reply_admission` | 0 | 24 passed, 0 failed | `/tmp/grow-ask-reply-retry-regress.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline turn_completion` | 0 | 24 passed, 0 failed | `/tmp/grow-ask-reply-retry-regress.log` (appended) |

Coverage includes:

- Real failed-send recovery, automatic Unknown no-send, real Check action, Unknown/Absent same-body/same-ID retry, rapid coalesced click, late Unknown, changed-choice refusal, and unrelated composer draft preservation.
- Actual retry → replay PR metadata for Submitted/Allowed/Consumed/Handled/Blocked/Dismissed → canonical query, without a new terminal marker or notification; retained immutable text and canonical-only Accepted/Rejected projection.
- Invalid replay metadata and another real foreground cannot create a false terminal; Submitted/read/transport/decode errors do not grant sending.
- Replayed retry settled as Handled or Blocked unblocks exactly one subsequent ordinary FIFO input with its own new identity, without replaying the original reply.
- Ended child/cross-tab source routes its real failed-send Check retry and subsequent completion observation to the owning primary.
- Existing U1/U3, identity/epoch/source, terminal, Goal, stash and first-wins completion regressions remain green.

Scoped git diff --check and source self-review passed. After the final test run only a clarifying comment and this verification record changed; executable source is the tested version.

## Remaining limits

These are actual Pager dispatcher/QuestionView/ACP-notification entry tests with synthetic PR metadata, not a rebuilt CLI or a live end-to-end Shell↔Pager transport run. F3's server-side idempotency integration remains separately owned. Final fresh product/PTY and Windows checks were not run here. No fmt, commit, real user config/session/Goal changes, subdelegation, global parser rewrite or workspace-wide testing occurred. Parent retains independent review and feature-wide milestone acceptance.
