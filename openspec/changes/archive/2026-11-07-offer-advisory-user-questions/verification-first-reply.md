# U3 first advisory reply — implementation and verification

Status: U3 implementation and targeted regression verification complete; awaiting parent review. All commands started by this task have exited. Cargo and source ownership are released after this report. U1 normal-completion observation and U2 reachable frozen-payload retry remain separate known work; this record does not accept the whole feature or claim fresh CLI/PTY success.

## Implemented U3 behavior

Existing-session missing input intentionally remains durable UnknownUnconfirmed. The Pager consumer now permits a local first editor on a matching durable UnknownUnconfirmed or Absent only when no frozen queue/in-flight payload exists. Available means local editing is possible, not canonical absence or acceptance. Submitted, timeout, unavailable, invalid evidence, transport/decode failures, mismatched evidence, and existing frozen payload do not open a new editor. Existing terminal card/in-flight guards remain.

The first explicit Submit fixes its literal full body and `question-reply:{source}:{call}` identity, puts a ready row into the existing normal FIFO, and uses its existing checkpoint/F3 atomic identity-and-payload admission path. It no longer waits for a query to prove global absence. Duplicate local submissions retain the original body and existing observation behavior. No card payload copy, field, registry, wire, backend, Shell/Tools/Goal change, or new completion/retry hook was added.

If the original turn is busy, the first row waits in normal FIFO. Unknown or other noncanonical/error observations cannot turn this unsent ready row into review-only. Existing failed/uncertain review rows stay frozen; Submitted and terminal observations retain their canonical handling. The original turn's real PromptResponse completion drains the original queued answer and ID.

This task changed only `dispatch/prompt.rs`, `dispatch/tests/advisory_reply_admission.rs`, and this record. Existing observer qualification in `dispatch/task_result.rs` was sufficient and was not edited for U3. Other outstanding worktree changes predate this task and are preserved.

## Actual old red → new green

All Cargo invocations used `PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`, Rust `+1.93.1`, `--locked --offline`, and background execution with no wrapper timeout.

1. Before any U3 production edit:
   `cargo +1.93.1 test -p pager --lib --locked --offline u3_`
   exited **101**, **2 passed / 2 failed**. The first-editor test failed because legal durable Unknown did not open the editor. The busy-first-submit test failed because initial Submit returned a query instead of enqueueing ready input. Full log: `/tmp/grow-ask-first-reply-oldred.log`.
2. The initial fix run reached **3 passed / 1 failed**. The remaining busy test fixture supplied an Ok PromptResponse without required `meta.promptId`, which the production first-wins finalizer correctly ignores. The fixture was corrected to the actual protocol; the production finalizer was not changed. This intermediate run's log was superseded by the final green log below.
3. Final same targeted command exited **0**, **4 passed / 0 failed**. Full log: `/tmp/grow-ask-first-reply-green.log`.
4. `cargo +1.93.1 test -p pager --lib --locked --offline advisory_reply_admission`
   exited **0**, **14 passed / 0 failed**. Full log: `/tmp/grow-ask-first-reply-regress.log`.

The four U3 tests cover:

- Actual QueryAdvisoryReplyAdmission → matching durable Unknown → visible local QuestionView editor → actual QuestionView submit action → ordinary SendPrompt with the complete literal question/answer/source body and stable ID; original composer draft is restored, with no optimistic Accepted claim.
- Unknown with either an existing frozen queue or uncertain in-flight body never opens a new editor.
- First explicit busy submission keeps a ready FIFO row after Unknown, timeout, unavailable, invalid evidence and transport error, then the original turn's actual PromptResponse with matching prompt metadata drains the same answer and ID.
- Submitted and timeout/unavailable/invalid/transport/decode error paths without payload never open the editor or send.

The original ten tests were adapted only where the revised first-Submit contract now emits SendPrompt directly. Terminal, immutable original body, literal `/exit`, owner/session/epoch/source qualification, ended child → primary across tabs, paused/stopped Goal preservation, and composer/doctor stash assertions remain and passed. The old direct-Submit retry test remains regression evidence for its existing reachable dispatcher only; it does not claim U2's real Check UI retry is fixed.

## Scope and remaining gates

Scoped `git diff --check` passed and the U3 production diff was self-reviewed. No fmt, CLI rebuild, PTY, full-workspace test, commit, real user configuration, or real session manipulation was performed.

U1 must still connect normal terminal completion to canonical observation, and U2 must independently repair the actual explicit frozen-payload retry entry. Full fresh CLI/provider/PTY verification remains with the parent milestone after U0/U3/U1/U2 combine. The four/14 passing local tests establish the bounded U3 change, not those separate product outcomes.
