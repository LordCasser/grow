# U5 automatic Absent — implementation and verification

Status: bounded U5 fix and local verification complete; pending parent review. All commands have exited and Cargo/write ownership is released with delivery. This does not accept the full advisory feature or claim final CLI/PTY success.

## Root cause and minimal fix

Read `review-auto-absent.md` and checked the real failure restore chain. First Submit → Send → nonempty ordinary composer → PromptResponse Err stores the immutable reply as a queue-only `requires_review=true` row, without in-flight. The automatic qualified durable Absent callback used to clear review and drain that answer even without a current explicit Check intent. Missing session storage can produce this canonical Absent; ordinary existing-session Unknown semantics do not rule it out.

The U5 production diff is limited to `reconcile_advisory_reply` in `dispatch/prompt.rs`:

- Only an already-qualified explicit `retry` clears a review flag. Noncanonical observations without active in-flight preserve the row's previous qualification: ready first input stays ready, failed/uncertain input stays review-only. Submitted/active observations still freeze where required.
- Automatic Absent can attempt normal drain only for a matching row that is actually ready. It cannot gain drain eligibility from merely having a queued row. Canonical-terminal successor FIFO wake and replay consumption remain unchanged.
- Removed the now-unused `had_queued` lookup. No field, state, wire, registry, backend or framework was added.

Only `prompt.rs`, its existing `tests/advisory_reply_admission.rs` module and this record were changed for U5. Other files and pre-existing work were preserved.

## Actual old red

All Cargo commands used `PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`, Rust +1.93.1, `--locked --offline`, background execution and timeout 0.

Before U5 production changes:

```sh
cargo +1.93.1 test -p pager --lib --locked --offline u5_
```

Exit **101**, **0 passed / 1 failed**. The real-entry regression failed at `automatic durable Absent must not release a failed review row`. Full log: `/tmp/grow-ask-auto-absent-oldred.log`.

The test establishes queue-only review via actual first Submit → Send → nonempty ordinary composer → actual PromptResponse Err. It explicitly asserts no in-flight snapshot, review remains true, original full text/ID and unrelated composer remain intact. Both explicit Unknown and Absent then go through actual QueryAdvisoryReplyAdmission to retry the original body/ID once; rapid clicks coalesce and late automatic callbacks do not duplicate sending. No manual review flag or Idle assignment simulates the failure state.

## Final green

| Command | Exit | Result | Full log |
| --- | --- | --- | --- |
| `cargo +1.93.1 test -p pager --lib --locked --offline u5_` | 0 | 1 passed, 0 failed | `/tmp/grow-ask-auto-absent-green.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline advisory_reply_admission` | 0 | 25 passed, 0 failed | `/tmp/grow-ask-auto-absent-regress.log` |
| `cargo +1.93.1 test -p pager --lib --locked --offline turn_completion` | 0 | 24 passed, 0 failed | `/tmp/grow-ask-auto-absent-regress.log` (appended) |

The first full-suite attempt passed 24 and failed one old direct-Submit retry expectation. Parent inspected the actual production caller and confirmed frozen replies can retry only via the actual Check one-shot intent; a later direct Submit may be a duplicate stale action. That test had relied on the same automatic Absent eligibility disproved by U5. It now retains the changed-choice input attempt, asserts original body/ID/review remain and automatic Absent sends nothing, then requires actual Check → Absent before the same original body/ID sends. Immutable text and identity assertions were strengthened, not removed. No implicit retry intent was added to production.

The final full suite retains U3 ready-first busy FIFO progression, U2 explicit Unknown/Absent retry and active/error/terminal guards, replay observation consumption, canonical-terminal ordinary successor wake, child/primary cross-tab routing, immutable body, canonical source/epoch identity, Goal and stash checks. Existing first-wins turn completion regressions pass.

Scoped `git diff --check` and source self-review passed. No executable source changed after the final passing test run.

## Limits

The new regression drives actual Pager dispatch/recovery/callback entry points with a qualified synthetic durable Absent response. It does not simulate real session-directory deletion end to end. No CLI rebuild, PTY, Windows, full-workspace test, real user state, commit, fmt or subdelegation was performed. Parent retains independent review and final product acceptance.
