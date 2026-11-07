# Advisory questions — focused editor product verification

## Status

The fresh CLI build passed, and both existing PTY cases passed together on their first run with the U6-only Tab removed: 2 passed / 0 failed, actual exit 0. The one complete mouse main scenario failed at the reply body visibility barrier, with provider and canonical completion now captured. It is not accepted as a passing mouse flow. No retries or production changes followed. Earlier no-Tab U6 failure, workaround pass, F3 peer pass and historical mouse marker failure remain preserved in `verification-final-product.md` and `/tmp/grow-ask-final-*`.

All three owned long commands have actual terminal states. At `2026-11-06T16:22:37.220357+00:00`, no owned Cargo/PTY executable remained, and production source and binary hashes matched the fresh-build identities. Cargo and the assigned test/document write scope are released after this record; overall feature acceptance remains with the parent.

## Scope and preparation

This executor changed only `pager/tests/pty_e2e/advisory_questions.rs` and this new record. The existing module registration is retained. No production source, shared harness, other tests, Cargo configuration, historical verification records or real user state was edited.

The U6-only Tab after opening Reply was removed, together with its workaround comment. Real Reply must now immediately own Down/Enter, second-question navigation, Other input and submission. Existing ordinary composer/history viewport actions, canonical assertions and cold replay behavior remain unchanged. A diagnostic checkpoint now records the real Submit aftermath; a reply body-marker timeout records actual screen/RPC/provider/native Timeline/input blobs before unwinding the sandbox.

The current source shows the intended repair only after successful `open_advisory_reply`: when that source view is still Scrollback, it forces the Prompt pane. Refusal paths return before focus changes. This test does not alter inbound advisory behavior or manufacture any presentation/admission acknowledgement.

## Source identity and build

- HEAD: `8ab486a6e45870561b91dba872934a6e24a795f1` with shared dirty implementation present.
- Tracked production diff SHA-256: `2db52b9e210f27d363a5034f69a982351c764fbbdbb7f666e2b453e7db8760de` over Pager/Shell/Tools source, CLI, Cargo.lock and root Cargo.toml.
- Source identity record: `/tmp/grow-ask-focused-source-identity.json`, recorded `2026-11-06T16:17:05.996771+00:00`.
- Parent-designated fresh CLI: `/Users/lordcasser/workspace/projects/grow/target/debug/grow`.
- Fresh build actual result: exit 0, 102.27 s; completed `2026-11-06T16:18:47.999465+00:00`.
- Binary bytes: `424532712`; mtime: `2026-11-06T16:18:47.807573+00:00`.
- Binary SHA-256: `9c6446f5f5a51f30cbd13ff0709e0914cec649adeb3a4d2eb8855a01e43a9f8e`.
- Build log: `/tmp/grow-ask-focused-cli-build.log`; binary metadata: `/tmp/grow-ask-focused-binary-identity.json`.
- Binary BLAKE3 from both actual PTY processes: `f222d1fb36b518245dada0951840cb71d6b25ec5cc2e06609e7bc63da64a10a1`.

All commands run from repository root with Rust `+1.93.1`, `PATH="$HOME/.cargo/bin:$PATH"`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2` and command-local removal of CARGO_TARGET_DIR. The build also removes PAGER_BINARY; PTY receives the absolute designated path only after actual build exit 0. No prior artifact fallback is allowed.

```sh
cargo +1.93.1 build -p cli --bin grow --locked --offline
PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline advisory_questions:: -- --ignored --test-threads=1 --nocapture
```

## Actual PTY results

| Command | Actual result | Full log |
| --- | --- | --- |
| Both existing `advisory_questions::` cases | 2 passed / 0 failed, exit 0; 58.68 s command / 27.86 s tests; completed `2026-11-06T16:20:15.916527+00:00` | `/tmp/grow-ask-focused-pty-product.log` |
| One complete mouse main case | 0 passed / 1 failed, exit 101; 38.57 s command / 37.29 s test; completed `2026-11-06T16:21:14.918365+00:00` | `/tmp/grow-ask-focused-pty-mouse.log` |

The default run explicitly removed `GROW_PTY_REPLY_MOUSE_PROBE` so it exercised the real visible `r` keyboard entry. The mouse command used the same absolute PAGER_BINARY and exactly the main test filter with `GROW_PTY_REPLY_MOUSE_PROBE=1`. It set `GROW_PTY_CAST_DIR=/tmp/grow-ask-focused-pty-casts/mouse-probe` to preserve the keyboard evidence independently. Only one run of each command was performed; no assertion was removed or weakened and no navigation sequence was changed in this round.

```sh
GROW_PTY_REPLY_MOUSE_PROBE=1 PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline advisory_questions::advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only -- --ignored --test-threads=1 --nocapture
```

The keyboard main proved the complete chain without editor-focus Tab: real questions and exact recommendations visible; no-human-answer continuation and explicit NOT-answered tool result; editable ordinary composer while the original Turn runs; durable original Turn end; immediate real Down/Enter from `r`-opened editor; changed `/quit` selection and `/new LATE_REPLY_NOTES_LITERAL` Other notes; one new literal FIFO input retaining original question/recommendation/source and human attribution; one stable reply Submitted/Consumed with blob BLAKE3 verification; one old call completion and model result; automatic live `Answered — read-only` without another Reply query; cold continue to the same session, read-only click, no extra provider request, duplicate submission or new session. The source session for this pass is `01a1b1a8-daa6-7263-97d6-1b57ec3cfa4b`.

The Settings case passed through the real F2 row and saved TOML false, preserving seeded timeout and unrelated raw fields. It observed the same live session and a cold continued session using real blocking questions despite `blocking=false`, saw no continuation in the bounded window after the visible UI barrier, and released only on a real changed selection. Both calls had one canonical settlement. It did not invoke the Shell setter directly.

## Mouse failure and preserved post-submit evidence

The mouse Reply body double-click opened the real QuestionView. With no Tab workaround, direct Down/Enter advanced to the second question, Other notes were edited and the real submit was sent. The first failure remained the reply body marker: `ADVISORY_REAL_REPLY_TURN_DONE` was not visible within 30 s. The failure screen shows the new literal reply bubble and `Worked for 0.3s`, without that assistant body. No second mouse run, alternate click, new session or production fix followed.

This round closes the previous evidence gap. The checkpoint saved immediately after Submit and the checkpoint saved before body-timeout panic both include native Timeline, referenced input blobs, foreground provider requests, actual ACP RPC log, unified log, screen and cast:

- `/tmp/grow-ask-focused-pty-casts/mouse-probe/advisory_reply_submitted.*`
- `/tmp/grow-ask-focused-pty-casts/mouse-probe/advisory_reply_body_timeout.*`

The timeout artifacts establish:

- Source session: `01a1b1a9-924c-7791-a669-c536a62573dd`; stable prompt id: `question-reply:01a1b1a9-924c-7791-a669-c536a62573dd:call_pty_advisory_late_reply`.
- Foreground provider requests: exactly 3. The third includes original questions, Agent recommendations, real `/quit` answer, slash-style Other notes, and original source/call identity. Old-call results per request are `[0, 1, 1]`.
- Checksummed Timeline decoding succeeds in `record_checkpoint`. It records two Submitted events total (original prompt and one reply), the stable reply's own Turn and Consumed event, and the reply Turn ending `completed`.
- Canonical domain sequence 62 contains the assistant item `ADVISORY_REAL_REPLY_TURN_DONE`; the body exists durably even though this real screen does not display it.
- The actual RPC log contains `session/prompt` success with `stopReason=end_turn` for the same stable id, followed by the automatic query returning `source=durable`, `status=consumed`, FIFO route.

Therefore this failure is not evidence of missing provider completion or missing durable input acceptance. The saved mouse terminal recording has 104 output events and no occurrence of the complete reply marker, while the keyboard main recording contains it at 5.0715 s and 5.6651 s. This narrows the observed difference beyond a single final screen snapshot; the recordings do not expose Pager's internal fold state.

A source candidate remains separate from those established facts: the actual mouse path calls `handle_scrollback_click` before emitting the advisory query, and `selection.rs` toggles a foldable selected entry on the second click. The mouse editor-open frame shows the original card collapsed while the real question editor owns keys. Whether that fold/selection state suppresses the later assistant body is not established. No single fixture navigation correction follows from the preserved artifacts, so the body-visible assertion was retained and no retry was made. The complete mouse path is **not** accepted; the default keyboard/Settings pass remains independently valid. Earlier historical mouse failures are not relabeled as fixture errors.

## Isolation, cleanup and handoff

Every checkpoint and cast lives under `/tmp/grow-ask-focused-pty-casts`, with the mouse subdirectory separated. The main run additionally preserves `advisory_live_answered.*`, `advisory_cold_answered.*`, `advisory_reply_main.cast`, `advisory_reply_continue.cast` and `advisory_settings_continue.cast`. Screens and durable references provide actual UI and canonical evidence; no fabricated handler receipt is used.

All three long commands were run with background=true and timeout=0, and actual terminal exits were retrieved. The full build/test logs were read from their beginnings, including the first mouse failure. At `2026-11-06T16:22:37.220357+00:00`, no owned Cargo/PTY executable remained. The production diff hash was still `2db52b9e210f27d363a5034f69a982351c764fbbdbb7f666e2b453e7db8760de` and binary SHA-256 was still `9c6446f5f5a51f30cbd13ff0709e0914cec649adeb3a4d2eb8855a01e43a9f8e`. Existing real user config/session/Goal and user/leader processes were untouched.

Cargo and the assigned test/document write scope are released. The parent may review the successful no-Tab keyboard path and assign the preserved mouse display failure separately. Scoped whitespace checks emitted no diagnostics; `git diff --no-index --check` returned 1 because these files are new content. These checks do not replace the actual runtime evidence. This task did not run F3 again, any full suite, formatter, archive/acceptance, commit, push or release command.

## Verified coverage and remaining boundaries

The two existing cases cover actual provider ask → visible recommendation → unanswered continuation → original durable Turn end → visible Reply → changed human selection and slash-style Other notes → ordinary new input with original source/recommendation/human provenance → unique stable Submitted/Consumed and input blob hash → single old-tool result → live Answered without another Reply query → cold continue read-only/no-new-request/no-new-session. Settings uses actual F2 saved false in same-alive and cold forced blocking controls. All prior behavior assertions remain.

The existing test-local `GROW_PTY_REPLY_MOUSE_PROBE=1` exercised the complete main case exactly once through a double-click on the visible Reply body, with unchanged text selection settings. Separate artifacts preserve the keyboard evidence. Its unresolved display failure is reported above; there was no blind retry or production fix.

Each Pager uses the existing ContentController/TestSandbox, isolated HOME/GROW_HOME and temporary project, loopback provider, `--no-leader --trust`, and explicit removal of ask preference/timeout environment overrides. Native Timeline is decoded through `shell::session::testkit::decode_timeline_fixture`; referenced input blob bytes are hashed. No real API/session/Goal/leader is used.

The prior same-binary F3 replay test was accepted before this Pager-only focus repair and is not rerun here. Unknown ACK/fault injection, child-to-primary routing, StoppedGoal boundaries, Plan regressions and the separate original late blocking stall are not accepted by this bounded round. Feature review, OpenSpec lifecycle and release remain parent-owned.
