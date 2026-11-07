# Advisory questions — real Pager PTY verification

## Status

Partial verification; the product chain is blocked, not accepted. Fresh CLI build passed. Of the two feature cases, Settings passed in the real same-alive and cold-continue path; the primary late-reply case fails before its editor opens. The final diagnostic run was 0 passed / 1 failed, exit 101, at `2026-11-06T14:41:36.539119+00:00`.

The real RPC evidence establishes the current first cause: the keyboard Reply action sends the correct source session/stable prompt identity, but the actual runtime returns `Method not found: "unknown ACP extension method: grow/input/admission_status"`. The UI consequently displays `Reply status unknown` and never opens QuestionView. This result does **not** prove that the Shell's intentional `UnknownUnconfirmed` producer branch ran; the method did not reach that reader. No bounded-read error appears in the captured query evidence. The post-submission live `Answered — read-only` assertion, immutable reply input checks, and replay read-only assertions remain intact but were not reached.

All six owned command tasks (one build and five test runs) reached actual terminal states. At `2026-11-06T14:42:48.442768+00:00`, the tracked process ids had exited, the production diff hash still matched the build source, and the CLI SHA-256 still matched the fresh artifact. Cargo and the three assigned test/document files are released after this record; no further test retry or source edit is owned by this task.

The PTY task has changed only:

- `crates/codegen/pager/tests/pty_e2e/advisory_questions.rs`
- `crates/codegen/pager/tests/pty_e2e_shell_tools.rs` (module registration)
- this verification record

No production code, Cargo files, shared harness, other tests, real user configuration/session/Goal, commit, or OpenSpec lifecycle state was changed by this task.

## Actual commands and results

All commands ran from repository root with Rust `+1.93.1`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, `PATH="$HOME/.cargo/bin:$PATH"`, and command-local removal of CARGO_TARGET_DIR. The build also removed PAGER_BINARY. Tests explicitly used the designated fresh path and `/tmp/grow-ask-pty-casts`.

```sh
cargo +1.93.1 build -p cli --bin grow --locked --offline
cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline advisory_questions:: -- --ignored --test-threads=1 --nocapture
```

The build exited 0 in 108.52 s; full log: `/tmp/grow-ask-pty-cli-build.log`. Later diagnostic reruns used the same test command with the exact `advisory_questions::advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only` filter to avoid repeating the passing Settings case.

| Run | Actual count and exit | Full log | Interpretation |
| --- | --- | --- | --- |
| First | 0 passed / 2 failed, exit 101 | `/tmp/grow-ask-pty-product-first.log` | Fixture counted only diagnostic foreground headers; BYOK requests have the mock classifier's tools-list fallback. Fixed to match the existing mock classifier. |
| Second | 0 passed / 2 failed, exit 101 | `/tmp/grow-ask-pty-product-second.log` | Initial card action had already become Unknown; Settings same-alive path completed but cold setup reused an expectation name. Made expectation names call-specific. |
| Third | 1 passed / 1 failed, exit 101 | `/tmp/grow-ask-pty-product-third.log` | Settings fully passed, including cold continuation and canonical single settlement. Primary Reply double-click folded the card rather than opening the editor. |
| Fourth, primary only | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-pty-product-fourth.log` | Actual visible `r` Reply action changed the card to Unknown; editor never opened. |
| Fifth, primary only with existing ACP debug logging | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-pty-product.log` | Same keyboard behavior, with the actual method-not-found RPC failure captured. Final source compiled successfully. |

The primary scenario did prove real ask emission, actual visible questions/recommendations, no-human-answer continuation and its explicit model result, ordinary composer usability during the Turn, normal final body, canonical original Turn end, and one canonical old-tool completion/result. It did not reach real edit/submit, stable reply Submitted/Consumed, post-submit card confirmation, or cold reply replay. The passing Settings case actually decoded canonical Timeline via `shell::session::testkit::decode_timeline_fixture`; provider history was not substituted for durable settlement checks. The reply payload BLAKE3 and stable identity assertions are prepared but unreached.

## First Reply failure evidence

The final isolated source session is `01a1b14e-5a97-7d51-a464-faff64820dbd`; the requested id is `question-reply:01a1b14e-5a97-7d51-a464-faff64820dbd:call_pty_advisory_late_reply`.

`/tmp/grow-ask-pty-casts/advisory_initial_reply_unknown.rpc.log:19-20` records the actual gateway exchange:

```text
sending "ext_method" request: {"sessionId":"01a1b14e-5a97-7d51-a464-faff64820dbd","promptId":"question-reply:01a1b14e-5a97-7d51-a464-faff64820dbd:call_pty_advisory_late_reply"}
received "ext_method" error: Method not found: "unknown ACP extension method: grow/input/admission_status"
```

Only safe test request identity/error fields are quoted here, not environment/header dumps. This disproves the earlier static hypothesis that the current UI directly received the reader's `unknown_unconfirmed` value. `session_admin.rs` contains a handler and intentionally treats a missing Submitted record in an existing session as unknown; changing that generic rule is not authorized by this PTY task, and the actual missing outer method dispatch must be resolved first.

Artifacts under `/tmp/grow-ask-pty-casts` include `advisory_original_ended.*` and `advisory_initial_reply_unknown.*`: cast, screen, foreground provider requests, native checksummed Timeline, referenced immutable input blobs, ACP RPC log and isolated unified log. The test's existing decoder successfully read the captured Timeline during checkpoint capture. The failure screen visibly keeps the exact questions/recommendations and composer draft, shows `Reply status unknown`, and never shows `/quit` in QuestionView. `advisory_settings_continue.cast` preserves the passing control path.

The mouse behavior remains a separate unclosed observation: expanding/selecting the real Reply card then double-clicking its visible Reply row can fold the card without opening editing. The final primary test uses the visible keyboard `r` route to distinguish the query failure. No successful mouse Reply admission is claimed, and no production mouse code was changed.

## Prepared product-chain coverage

`advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only`:

1. A loopback `ContentController` provider emits an actual `ask_user_question` tool call with `blocking=false`, two questions, and exact legal `recommended_option` labels. Explicit call-id SSE fixtures are the existing `expect_tool_turn` helpers. No ACP handler reply or tool output is fabricated by this test.
2. The next foreground provider request reaches its real completion barrier without any human answer. The actual Pager tool card is expanded through SGR mouse input if collapsed; the question and `agent recommendation: /new` must be visible. The provider result must retain the question/recommendation and `the user has NOT answered`; every request containing the original call has exactly one result for it.
3. While that original Turn is still running, a separate ordinary composer draft must be editable. Releasing the provider completion barrier yields the normal final response. The checksummed canonical Timeline must record that original Turn's end before any Reply action occurs.
4. The test selects the actual visible Reply card with SGR mouse input and invokes its displayed `r` keyboard action. The prepared remainder uses the existing QuestionView to change the first option to `/quit` and edit the second question's Other notes to `/new LATE_REPLY_NOTES_LITERAL`, then really submit. The next foreground provider request must contain the actual human selection, notes, original questions, agent recommendation, source session, and original call id. The independent composer draft must be restored. Current execution stops before the editor opens.
5. Canonical Submitted events are matched through their referenced immutable input payload, with the payload BLAKE3 checked. Exactly one Submitted may carry `question-reply:{source_session_id}:{tool_call_id}`; it must have human origin, FIFO requested route, no Skill invocation, a distinct later Turn, and real input consumption after the original Turn ended. The original tool's canonical completion and model-visible result remain one each. The live session-directory set must not change because a label resembles `/quit` or `/new`.
6. After a clean process exit, a new isolated Pager process uses `--continue`. The card's real Reply action reconciles its stable identity, becomes `Answered — read-only`, and a subsequent card click must leave the ordinary composer usable. A bounded observation window must see no new foreground provider request, no second Submitted, and no second old-call settlement.

`settings_saved_false_forces_next_same_alive_and_cold_ask_to_wait_for_real_answer`:

1. Complete one real user Turn, open Settings with F2, search the actual Agent row `Allow non-blocking questions`, and toggle it through the existing keyboard action/effect path.
2. Wait for the isolated saved TOML to contain `allow_non_blocking=false`; verify seeded timeout fields and an unrelated raw field survive. The test does not call the Shell setter directly.
3. In that same live session the provider still requests `blocking=false`. A real QuestionView and its alternative option are the causal barrier. During the following 900 ms bounded observation window no next foreground provider request may arrive. A real option selection/Enter must then release the continuation, whose result names the human answer.
4. Cleanly exit and cold `--continue` the same isolated session. A new `blocking=false` ask must still use the actual blocking QuestionView, remain held through the same observation window, and continue only after a real answer. Both calls must have one canonical completion/result.

## Isolation and artifact identity

Every test uses the existing `ContentController`/`TestSandbox`, isolated HOME and GROW_HOME, and a fresh temporary git project. Provider configuration is seeded only with the loopback URL. Pager runs with `--no-leader --trust`, preserving the existing sandbox's external-discovery isolation. Each spawn explicitly removes `GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING`, `GROW_ASK_USER_QUESTION_TIMEOUT_ENABLED`, and `GROW_ASK_USER_QUESTION_TIMEOUT_SECS` after the sandbox baseline, so an inherited environment override cannot masquerade as a saved preference.

The test requires an explicit absolute `PAGER_BINARY`; it never calls the helper fallback that may pick/build another artifact. The parent designated `/Users/lordcasser/workspace/projects/grow/target/debug/grow`; it was used only after the actual fresh build returned exit 0. No prior PAGER_BINARY was inherited. CARGO_TARGET_DIR was absent and was explicitly removed from the build/test command environment; global configuration was not edited.

| Evidence | Current value |
| --- | --- |
| Parent-designated final binary | `/Users/lordcasser/workspace/projects/grow/target/debug/grow` |
| Build completion time | `2026-11-06T14:26:33.630250+00:00` |
| Binary mtime | `2026-11-06T14:26:33.450079+00:00` |
| Bytes | `424491496` |
| SHA-256 | `2f51b7e931546b1101809f727b3d7d8cd9a5a2ffe660f2c1145e6d3c64579dbf` |
| Binary BLAKE3 | `85d6d89e480d25c0eb4da376cf9a8be15bcbb2c36f6c011a0c873fb78d3ecace` |
| Test process result / log / casts | Settings passed; primary failed; all processes exited. `/tmp/grow-ask-pty-product.log`, `/tmp/grow-ask-pty-casts` |

Source HEAD was `8ab486a6e45870561b91dba872934a6e24a795f1`, with the shared uncommitted implementation present. `/tmp/grow-ask-pty-source-identity.json` records the tracked diff SHA-256 `a9dccb9790767529bce438f990ecbcc01f636fc81e32456d1904bc91b8ea8d4d` over Pager/Shell/Tools source, CLI, Cargo.lock, and root Cargo.toml. This is a dirty source identity, not a clean-commit claim. `/tmp/grow-ask-pty-binary-identity.json` records the artifact metadata above.

The optional `GROW_PTY_CAST_DIR` path preserves actual checkpoint/Settings casts. It also enables the existing command-local GROW_LOG_FILE/ACP gateway debug path under the isolated HOME so a failing test saves the actual query exchange before TempDir cleanup. Provider expectations, visible text, durable original-Turn end, and canonical payload references provide barriers; the 900 ms bounded windows observe product behavior after those barriers. No sleep or polling was used to await build authorization.

## Checks actually performed

- Loaded `software-engineering`, its Rust principles and handoff contract; read repository AGENTS, OpenSpec index, archived client-surfaces/input-admission contracts, this change's design, implementation handoff, and milestone verification plan.
- Confirmed there are no nested AGENTS files. Read the existing real PTY harness, explicit call-id provider fixtures, `ctrl_c_cancel_during_stream_recovers_cleanly`, `auto_wake_cancel_preserves_queued_user_prompt`, Settings toggle examples, and canonical Timeline verification examples.
- Source review confirmed the existing test-support feature exposes `shell::session::testkit::decode_timeline_fixture`, the required sandbox spawn API accepts EnvOp removals, and the assigned test target already has all used dependencies. No harness/source dependency has been added.
- Scoped `git diff --check` for the tracked module registration passed (exit 0). Each new untracked file was also checked with `git diff --no-index --check -- /dev/null <file>`: both emitted no whitespace diagnostics; exit 1 is the no-index difference status for the newly added content. These are whitespace checks only, not compilation or runtime evidence.
- The actual fresh CLI build and all five real PTY commands above were run and their terminal exits were retrieved. Full logs were read from the beginning, including each first failure. Final test source compiled successfully in the diagnostic run. No fmt, independent review, full OpenSpec validation, archive, commit, push, or release command was run by this task.

## Execution handoff

Cargo and the assigned test/document scope are released. All owned background commands have actual terminal status; known PTY/build pids are gone. The final production source diff and binary SHA-256 were rechecked unchanged after execution. Existing longstanding user Grow processes were not touched.

Parent/source owners must resolve the actual missing `grow/input/admission_status` route and the initial-edit consumer contract, then verify the previously identified post-submit canonical confirmation paths. This task neither changes the generic Unknown/Absent contract nor bypasses it. A fresh CLI build is required after production changes before re-running the primary case and its currently unreached durable late-reply/replay assertions. The mouse double-click behavior requires separate actual-event-chain investigation; the current passing keyboard admission cannot be claimed because its first query still fails.

The bounded PTY scope does not verify unknown-ACK/write-failure injection, child-to-primary routing, StoppedGoal boundaries, Plan regressions, or the separate original late blocking stall. Post-submit confirmation and stable reply Submitted/payload hash/replay checks are also currently unverified because the first Reply cannot open its editor. Settings' actual full pass is preserved independently. This record does not mark the feature accepted or ready for the 2.6.2 release.
