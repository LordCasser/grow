# Shell child Sampling control verification

## Scope and candidate

This is the S candidate for parent review, on the shared dirty workspace based on `8ab486…`. Existing changes from other owners are retained. This delegation changes only the actor control tests in `crates/codegen/shell/src/session/actor/model_switch.rs` and this record; the previously restored production candidate remains in place. No Pager, Tools, InputAdmission, Workflow, manifest, schema, counter or registry changes are made here.

The production candidate uses one `native-sampling:{control_epoch}` client per actor incarnation, generation 0, and the existing locked `PendingStepControls.next_sequence` allocator. Model and effort tool admissions allocate only when their independent admission sender is present and the original intent is absent. Invalid effort consumes its sequence; hydration/recovery with no intent remains silent. Parent `queued` still means actor admission. Existing target ownership, incarnation and permission checks remain authoritative.

## Test fixture corrections

- Supersession application expects transient `Applying` followed by durable `Applied` for the latest host sequence.
- No-op expects `Pending → Applying → Applied`, retains the same host intent and durable terminal receipt, and has no switch message.
- The busy actor test explicitly closes its Step before applying pending controls. Before closing it, the test asserts `Pending`, unchanged current route, an unresolved finish response, and legal first sequence 0. It then verifies the ordinary terminal notice, host identity and durable receipt.
- One new test, `native_terminal_ui_persistence_failure_preserves_applied_receipt`, reuses `create_test_actor_ex_with_projection_error(..., None)` and the ordinary `NotificationSender.persistence_tx` boundary. It rejects exactly the attempted terminal `AppendUpdateDurablyAndAck` with permanent `InvalidData`. It verifies admission succeeded, route application completed, finish reports the terminal UI persistence error, live feedback stops at `Pending → Applying`, and the host terminal receipt is Applied with `ui_terminal_durable = false`. The local writer is aborted and awaited. There is no additional fault framework or helper.

The fixture's `projection_error` argument affects response admission, not terminal UI append (`tests/support.rs:798`). Passing an injected error there would not prove this terminal screen/persistence boundary. The test therefore preserves the existing Timeline ACK fixture and rejects the actual UI append through the existing sender.

## Commands and evidence

Every Cargo invocation uses the same prefix:

```sh
env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 test -p shell --lib --locked --offline
```

Actor filters are passed together after `--`, followed by `--nocapture`:

```text
native_tool_sampling_uses_host_control_identity_through_closed_step
repeated_native_controls_share_one_host_client_and_supersede_in_order
rejected_native_effort_consumes_sequence_without_colliding_with_next_selection
native_no_op_selection_keeps_receipt_without_switch_message
no_intent_sampling_application_stays_ui_silent
native_terminal_ui_persistence_failure_preserves_applied_receipt
```

Owner filter: `subagent_model_owner_requires_exact_incarnation_and_actor_admission -- --nocapture`. The existing wrapper runs 10 isolated child cases: queued, actor-rejected, receipt-closed, receiver-closed, caller-cancelled, target-cancelled, target-stopped, replaced, replaced-during-lock, cancelled-during-lock. The handoff's earlier count of 11 was corrected by the parent; no test is added to match that number.

Native filter: `native_subagent_model_tool_controls_the_child_without_waiting_for_its_step -- --nocapture`, setting `GROW_TEST_SUBAGENT_MODEL_CASE` for each case listed below and a separate temporary `GROW_HOME` for every invocation. Cases run independently and all results are collected even after a failure. No case named `noop` exists in the native fixture; no-op is covered by the actor control test above.

Final source SHA-256 values (captured after formatting, before the final regression; rechecked at release):

```text
22d9dad866a8083069a35d381282b7daa353a1d0b585cab92b7babc6bcfaeee9  actor/mod.rs
c04c2b6a63ed4852491277449cc7865653d8fe18ede8a65c803d0f33a36a7e92  actor/model_switch.rs
16eff127629934229088f93e62a4f903cc9f3f404989ee1ed30fd8cbaf8e75b4  agent/mvp_agent/tests/session_model_tests.rs
```

Final actual results:

| Check | Result | Evidence |
| --- | --- | --- |
| Actor control filters (final formatted source) | 6 passed, 0 failed; actual exit 0 | `/tmp/grow-subagent-control-child-regress-actor-final.log` |
| Original owner wrapper | 1 test passed; all 10 isolated child cases passed; actual exit 0 | `/tmp/grow-subagent-control-child-regress-owner.log` |
| Native positive paths | `model`, `effort`, `pending`, `both`, `none`, `null`, `rebuild`: each 1 passed, actual exit 0 | `/tmp/grow-subagent-control-child-regress-native-<case>.log` |
| Native rejection/security paths | `unknown-model`, `unselectable`, `hidden`, `unsupported`, `effort-unsupported`, `malformed`, `empty`, `forged`, `denied`, `permission-denied`, `hook-denied`: each 1 passed, actual exit 0 | Same per-case log pattern |
| Native natural-finish cases | `mailbox-model-after-finish`, `mailbox-effort-after-finish`, `finished-during-lock`: each actual exit 101 at `session_model_tests.rs:623`, `finalization keeps the exact actor alive` | Same per-case log pattern |
| Scoped Rust formatting | actual exit 0 | `rustfmt +1.93.1 --edition 2024 --config skip_children=true --check crates/codegen/shell/src/session/actor/model_switch.rs crates/codegen/shell/src/session/actor/mod.rs` |
| Scoped whitespace/conflict check | actual exit 0 | `git diff --check --` the three source files and this record |

The native total is **18 passed / 3 failed**, not 21 passed. The aggregate runner completed all 21 cases and returned actual exit 1 because the two mailbox cases failed (it excluded only the already known `finished-during-lock` from its aggregate status). Its full actual-exit index is `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b245-44dc-7610-9535-9f25e2efefdb/terminal/call_2ba7cd69b7ef48c2abe5e449b6f345d6.log`.

All three natural-finish cases fail on their shared prerequisite that the exact child actor is still present after real completion, before reaching their intended lock/mailbox control assertions. Only `finished-during-lock` has the supplied independent old-behavior baseline; this run does not claim a separate old-behavior result for the mailbox variants. Their later control assertions remain unverified. The parent has explicitly assigned this shared fixture/retirement investigation to a separate task; no lifecycle repair is included here.

The compiler emitted the existing irrefutable-pattern warning at `input_admission.rs:2528`; that unrelated owner file was not changed.

After the final regression, the three source hashes above were rechecked unchanged, and scoped formatting/diff checks passed again. The aggregate command has terminated, all owned Cargo/test processes have exited, and this delegation releases both Cargo and its Shell source ownership for parent review. No commands or monitors remain running.

## Preserved red/green evidence

The parent supplied and already inspected the original producer → tool → coordinator → child control evidence. It is retained without rerunning or overwriting:

- `/tmp/grow-subagent-control-child-oldred.log`: `CASE=both`, removing only the two host identity allocations, actual exit 101 after queued because Pending is missing.
- `/tmp/grow-subagent-control-child-oldred-actor.log`: corresponding actor old behavior produces zero control updates.
- `/tmp/grow-subagent-control-child-green.log`: production restored, `CASE=both`, actual exit 0 through provider/tool/coordinator, child Pending, closed-Step, durable `model.changed`/Applied notice and next provider route.
- `/tmp/grow-subagent-control-child-oldred-finished.log`: baseline `finished-during-lock` failure at `session_model_tests.rs:623`, `finalization keeps the exact actor alive`, actual exit 101. The assertion is retained. This independent fixture limitation does not establish a regression in host Sampling identity and is outside this patch's repair scope.

Local fixture iteration is also retained: `/tmp/grow-subagent-control-child-regress-actor.log` has 4 passed / 1 failed (actual exit 101; the test failed to close its Step); `/tmp/grow-subagent-control-child-regress-actor-v2.log` has 6 passed / 0 failed, actual exit 0 before final formatting and the strengthened no-op receipt assertion.

## Parent acceptance and retirement-fixture follow-up

Parent accepted the bounded Shell feature after inspecting the candidate and the actual six-control/owner/positive/security logs. The three natural-finish prerequisite failures were then repaired independently in [align-native-control-retirement-fixture](../2026-11-07-align-native-control-retirement-fixture/verification.md). The original 21-case native wrapper and 10-case owner wrapper now pass in full on the stable integrated source, alongside the added Model/Effort actor execution-closed control. No case was removed and production Result/shutdown ordering was retained. Final logs: `/tmp/grow-native-retirement-regress{,-owner,-actor}.log`, serial command actual exit 0. The earlier 18/3 table remains the honest result of that earlier run.

## Limits and review

This records Shell's bounded control candidate only. Parent source and actual-log review is complete; the retirement-fixture maintenance is accepted and archived. Pager row integration and the real CLI/PTY milestone are tracked separately. No broad Cargo check/build/workspace/CLI, commit, push, archive, release, or real user-state operation was performed by this Shell delegation.
