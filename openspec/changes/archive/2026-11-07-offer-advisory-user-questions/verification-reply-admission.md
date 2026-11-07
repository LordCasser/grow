# Task B — stable advisory reply admission

## Current acceptance

父已核最终输入接纳实现，并由原独立reviewer复核U1/U2：正常PromptResponse与durableTurnCompleted都在drain前按原完整身份查询；真实card Check只在合格Unknown/Absent、idle且无当前foreground/RPC时重送原冻结正文/ID。自动Absent不解冻失败row，terminal先于队列修改吸收stale观察，AdmissionReplay只退休精确本地提交占位。此次输入owner链无阻断发现，结论与边界见[最终复核](review-reply-final.md)。

实际`/tmp/grow-ask-editor-focus-green.log`的advisory filter为37 passed/0 failed，含U1/U2/U3/U5/U6及身份、FIFO、Goal控制；fresh CLI自然键盘主链与Settings为2 passed/0 failed，见[focused产品记录](verification-focused-product.md)。mouse正文首次显示仍由U7单独处理，不接受整个feature完成。

## Historical Task B draft

以下保留原十项source-ready交付及当时限制，不是最终实现/验证状态。其“Unknown不开放编辑”“Absent才可retry”等文字已由后续U1–U5的真实入口修复取代，最终契约以design、delta和最终复核为准。

## Status at original delivery

Source ready, not execution-verified. No Cargo, check, fmt, network test, real session, or user configuration command was run by this task. Task A owns the shared build window; parent review and the compiled targeted tests remain required before acceptance.

## Input-owner change

Only Task B's `app/root/dispatch/prompt.rs`, the existing `InputAdmissionStatusResolved` branch in `dispatch/task_result.rs`, and new `dispatch/tests/advisory_reply_admission.rs` were changed. Prior Settings success-result handling is preserved. Card storage, rendering, helpers, actions/router, Shell, Tools, and Goal production code remain with their owners.

- Actual `SubmitAdvisoryReply` locates source → owning primary and verifies the complete stable card identity `question-reply:{source}:{call}` through the frozen helper, without splitting colons. It first attempts the monotonic card projection transition to Checking; refusal exits before queue/in-flight mutations.
- A first submitted body is held in the existing review queue with its stable `prompt_id`; subsequent card submissions preserve that body. Every submission first queries canonical admission. There is no global reply registry or second card payload.
- The existing observer validates primary owner/session, binding epoch, response session/prompt, and durable source before passing canonical results to Task B. A cold card may be an observation target with no queue/in-flight row. A transport error without response evidence can only retain/freeze the bound card as Unconfirmed; mismatched response evidence has no effect.
- Canonical Absent releases an explicitly queued immutable body through the existing literal normal FIFO. Its stable ID excludes combination. With no frozen body and a consumed Reply-open intent, Absent calls the frozen editor-opening helper. Unknown, Submitted, Unavailable, TimedOut, InvalidEvidence, and errors never release a send or open the editor.
- Allowed/Consumed/Handled project Accepted and remove local duplicate queue rows. Blocked/Dismissed project Rejected, retain the original body as a selectable notice, and never reuse the terminal identity. Neither path writes another result to the historical tool.
- The monotonic card setter is checked before callback mutations; a previously confirmed in-flight disposition is also protected. Delayed Absent/unknown callbacks cannot reopen or resend a terminal card, including after its local in-flight row disappears.
- The existing F3/PromptResponse error path retains the immutable in-flight snapshot. Automatic reconciliation only observes it. An explicit Reply retry moves an idle uncertain snapshot to the frozen queue, then only a matching durable Absent permits the same text/id to be sent again. No automatic retry loop was introduced.
- Doctor fix may replace an AdvisoryReply local editor: restore its stashed original composer first, then stash for the doctor question. Ordinary blocking question refusal remains, and the local advisory replacement does not add turn pause duration.

## Test source prepared (not run)

The new test module is wired from `prompt.rs`; its test names can be selected with:

```sh
cargo test -p pager --lib advisory_reply_admission_tests
```

This command has **not** been executed by Task B. The source contains ten tests covering:

1. Actual Submit action → query → matching Absent callback → ordinary SendPrompt, fixed original body despite a changed choice, and literal `/exit` text.
2. Submitted/unknown/unavailable/timeout/invalid/transport error retain a frozen review body, never imply Accepted, and only query on retry.
3. Allowed/Consumed/Handled are read-only, with no second Send; delayed Absent/Submitted and a changed Submit cannot demote the terminal projection.
4. Actual in-flight Blocked/Dismissed do not reenter; selectable rejected-body text remains and later attempts cannot enqueue a replacement.
5. A cold card with no local in-flight/queue can observe canonical Handled and become read-only.
6. Wrong owner, epoch, response session, response prompt, or non-durable source has no effect.
7. Actual Submit → Absent → Send → `TaskResult::PromptResponse(Err)` → automatic query/Absent observation → explicit changed-choice Submit → Absent sends the original body and stable ID. This test uses the production failed-send recovery path rather than manually assigning Idle.
8. A finished child view's card on another active tab queries and sends to its owning primary; child and unrelated tab do not receive the input.
9. Actual `goal_updated` notification supplies the paused/stopped Goal projection; Submit/Absent produces only the ordinary SendPrompt and preserves that projection. The test does not mutate Goal production behavior.
10. Replacing a local advisory editor with doctor fix preserves the original composer stash and does not add turn pause time.

## Evidence actually collected

- Reused the loaded software-engineering entry, Rust and handoff references; read the Task B review and frozen A/B projection interface, then checked actual prompt, observer, queue, transport effect, card helper, F3 recovery, and relevant test fixtures.
- Scoped `git diff --check` passed for Task B's prompt/observer/test files (exit 0).
- Static inspection confirms Task A has now landed all four frozen AppView helpers and the `tool::AdvisoryReplyStatus`/card re-exports, including the monotonic setter's false-on-different-terminal contract. No interface gap was papered over with a default.

## Remaining verification / integration limits

All new tests remain uncompiled and unexecuted until the parent grants the shared Cargo window. Task A's real tool-output replay/click/presentation tests and parent-organized Shell/provider integration remain separate evidence: this task's cold-card fixture is not a claim that full replay passed. The module does not change tool result counts, but actual historical tool replay and transport milestone checks still require that separate integration. Existing F3 regressions and Task B's targeted suite should be run on the combined fixed source before feature acceptance.
