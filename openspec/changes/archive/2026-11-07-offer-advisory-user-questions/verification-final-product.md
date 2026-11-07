# Advisory questions — final real product verification

## Status

Partial execution; feature acceptance remains open. Fresh build passed. The Settings case passed again on this new binary. The first primary run exposed U6: invoking the real `r` action from scrollback opens a local advisory QuestionView without moving input focus, so Down/Enter still act on history and can open the previous response viewer. The parent explicitly classified this as a product UX gap and authorized a Tab workaround only to collect the remaining chain evidence. The original no-Tab failure is preserved in `/tmp/grow-ask-final-pty-product-first.log`; success with that workaround cannot establish U6 is fixed.

The primary case passed in an actual run (1 passed / 0 failed, exit 0, `2026-11-06T16:00:21.727346+00:00`). It proved actual changed selection/Other notes submission, new provider input with original source/recommendation/human provenance, one stable canonical Submitted/Consumed, referenced payload hash, old-tool single settlement, live Answered without another Reply query, and cold continue read-only with no new request/session/input. Settings passed on the same binary in the first run. F3 passed (1 passed / 0 failed, exit 0). U6 remains open: the passing primary includes the explicitly authorized Tab workaround and does not establish a complete defect-free user flow. The one bounded mouse observation opened the real editor through the actual query, but its subsequent whole-chain run failed at a missing reply body marker; mouse full-chain acceptance is not claimed. No more runs were started. Historical failed product runs remain in `verification-pty.md` and `/tmp/grow-ask-pty-*`; this record uses only the new `/tmp/grow-ask-final-*` artifacts.

Write scope is limited to `pager/tests/pty_e2e/advisory_questions.rs`, its necessary module registration, and this record. No production, shared harness, other tests, Cargo configuration, real user state, commit, push, or release changes are part of this task.

## Source and binary identity

- Source HEAD: `8ab486a6e45870561b91dba872934a6e24a795f1`, with the shared uncommitted implementation present.
- Tracked source diff SHA-256: `7d0c7f67e06c59cebabf449a4de501ed8bdb4cb4e0e7c38eda64b6e595f832f3` over Pager/Shell/Tools source, CLI, Cargo.lock and root Cargo.toml. Recorded in `/tmp/grow-ask-final-source-identity.json` before the build; this is a dirty source identity, not a clean-commit claim.
- Parent-designated binary: `/Users/lordcasser/workspace/projects/grow/target/debug/grow`.
- Fresh build completed: `2026-11-06T15:45:25.341191+00:00`, exit 0, 126.10 s.
- Binary mtime: `2026-11-06T15:45:25.085225+00:00`; bytes: `424532712`.
- SHA-256: `fb9d1df5686620595a01ec69437853f5dfd39d7781611869efbf8892a1578b0f`.
- BLAKE3 from the actual PTY test process: `7f1f4e76dd3d127e3691b6a7845fd331edc830e7279de124ca530701826ee7cd`.
- Metadata record: `/tmp/grow-ask-final-binary-identity.json`.

CARGO_TARGET_DIR and PAGER_BINARY were absent on entry and explicitly removed for the build. Only after actual build exit 0 was PAGER_BINARY set to the designated absolute path. No old artifact or fallback build was used.

## Commands and evidence

All commands run at repository root with `PATH="$HOME/.cargo/bin:$PATH"`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, and command-local removal of CARGO_TARGET_DIR. Long commands use the background task owner; final results require actual terminal exit.

```sh
cargo +1.93.1 build -p cli --bin grow --locked --offline
PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline advisory_questions:: -- --ignored --test-threads=1 --nocapture
```

Build full log: `/tmp/grow-ask-final-cli-build.log`. PTY full log: `/tmp/grow-ask-final-pty-product.log`. Optional diagnostic artifact directory: `/tmp/grow-ask-final-pty-casts`.

The F3 test's actual `test_support::grow_binary` implementation (`test-support/src/env.rs`) prioritizes `GROW_BINARY`; the real run used the same final binary through that variable. The existing peer test was not edited.

```sh
GROW_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo +1.93.1 test -p shell --test test_input_admission_replay_e2e --locked --offline test_prompt_admission_replay_real_acp_entry -- --ignored --test-threads=1 --nocapture
```

F3 actual result: 1 passed / 0 failed, exit 0, finished `2026-11-06T16:01:59.058657+00:00`, command duration 68.80 s. Full log: `/tmp/grow-ask-final-f3-peer.log`. It emitted the existing unused initial `timeline_path` assignment warning; no error. The test held a real foreground provider response, observed actual stdio ACP in-flight and post-completion canonical replay, rejected changed text with the same identity, and cross-checked no extra Submitted/Hook/Turn/roster/provider side effects against the durable journal. This is producer/consumer contract evidence, not PTY UI evidence.

## PTY run results and necessary fixture adaptation

| Run | Actual result | Full log / first failure |
| --- | --- | --- |
| First, both cases | 1 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-first.log`; Settings passed, primary exposed U6 before Other notes: scrollback Enter opened original body viewer. |
| Second, primary | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-second.log`; real reply admitted, card not visible after new Turn followed tail. |
| Third, primary | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-third.log`; draft Tab did not transfer navigation focus; bare g entered composer. |
| Fourth, primary | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-fourth.log`; explicit mouse selection focused transcript, but bare g is gated in ordinary non-vim mode. |
| Fifth, primary | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-fifth.log`; one Shift+Left selected current prompt, not preceding Turn. |
| Sixth, primary | 0 passed / 1 failed, exit 101 | `/tmp/grow-ask-final-pty-product-sixth.log`; live Answered passed, cold card had folded during selection/query, so status was hidden. |
| Seventh, primary | 1 passed / 0 failed, exit 0 | `/tmp/grow-ask-final-pty-product.log` and preserved `/tmp/grow-ask-final-pty-main-green.log`; complete late-answer and cold read-only path passed with U6 Tab workaround. |

No behavior assertion was deleted or weakened. Every failed command exited before a new one was started. Successful Settings evidence was not re-run after each unrelated main-fixture correction. The final primary key/barrier changes are: focus the already opened QuestionView using existing Tab (an explicit U6 workaround), wait on its navigation footer; preserve frame checkpoints through second question/Other; use actual simple-mode Shift+Left twice (response → current prompt → previous prompt) to inspect the live card without querying it; and restore cold card expansion only after its one canonical query. None of the navigation/expansion operations supplies another human answer or settles the old tool.

U6 source evidence remains `open_advisory_reply` creating the local editor while preserving Scrollback focus; `input.rs` routes question keys only when the pane is not Scrollback. A source owner must fix focus-on-open and remove the workaround for final acceptance. The first no-Tab failure is not reclassified as a fixture failure.

## Bounded mouse observation

A single diagnostic run set the test-local `GROW_PTY_REPLY_MOUSE_PROBE=1`; all default acceptance runs keep the visible `r` entry. This only changes the actual input gesture at the first Reply body; it does not modify text-selection configuration, the tool header, production code, or admission results. Log: `/tmp/grow-ask-final-pty-mouse.log`; separate artifacts: `/tmp/grow-ask-final-pty-casts/mouse-probe`.

The actual body double-click sent the correct stable-id query and received legitimate durable `unknown_unconfirmed` (`advisory_editor_open.rpc.log:19-20`), then displayed the real QuestionView (`advisory_editor_open.screen.txt` and second-question/Other checkpoints). Thus this bounded observation did not reproduce the old “only folds without query” symptom. It is insufficient to retroactively classify that earlier observation as a fixture error.

The reused whole-chain test later timed out waiting for `ADVISORY_REAL_REPLY_TURN_DONE`, showing the new human reply bubble and `Worked for 0.3s`, with no marker visible. Actual result: 0 passed / 1 failed, exit 101, finished `2026-11-06T16:03:27.965850+00:00`, command duration 57.32 s. No second mouse attempt or production fix was performed. The final post-submit runtime snapshot was not preserved before TempDir cleanup; the complete failure screen/log and pre-submit RPC/editor checkpoints remain. Consequently the bounded opening/query evidence is positive, but a fully green mouse product chain and the later missing-marker cause remain unverified. This does not invalidate the separately saved keyboard chain/F3 results, and does not close U6.

## Saved canonical and UI evidence

The successful run's source session is `01a1b196-d729-7143-b730-92cf59fdfd12`, with stable id `question-reply:01a1b196-d729-7143-b730-92cf59fdfd12:call_pty_advisory_late_reply`. The original and reply together have two canonical Submitted events and exactly one later Turn for the stable reply. Test assertions decode checksummed Timeline through the existing public testkit, validate immutable blob BLAKE3 references, assert reply FIFO route, user origin, empty skill invocations, one reply Submitted and consumed input, later sequence than original Turn end, one old tool completion and model result.

`/tmp/grow-ask-final-pty-casts` preserves original-Turn-ended, editor-open, second-question, Other-input, admitted, live-answered and cold-answered native Timeline/input blobs/provider/RPC/screens/casts. `advisory_live_answered.screen.txt` shows the original questions/recommendations and Answered beside the new reply Turn and restored composer draft; its actual RPC includes one initial durable Unknown query, one session/prompt with the literal reply body, and the automatic durable Consumed query. `advisory_cold_answered.*`, `advisory_reply_main.cast`, `advisory_reply_continue.cast`, and `advisory_settings_continue.cast` retain their respective real UI evidence. No handler receipt is substituted for an actual UI or canonical fact.

## Cleanup and handoff

All ten owned long command tasks have actual terminal status: one fresh CLI build, seven PTY fixture/main runs, one F3 peer run, and one bounded mouse run. At `2026-11-06T16:04:47.910259+00:00`, process inspection found no owned Cargo/PTY/stdio ACP executables remaining. The production tracked diff SHA-256 and CLI SHA-256 still matched their pre-build/fresh artifact records. Real user config/session/Goal and longstanding user Grow/leader processes were not touched.

Cargo and the assigned test/document write scope are released after this record. The parent may pass the stable production window to the U6 Card owner. The only test change in this continuation is within `advisory_questions.rs`; the existing module registration was retained, historical `verification-pty.md` was not rewritten, and the new record is this file. No production edits, source formatter, full suite, OpenSpec archive/acceptance, commit, push, or release was performed.

## Coverage boundaries

The primary case retains the original provider/card/Turn-end/actual Reply edit/changed choice/slash-style notes/stable input identity/immutable payload hash/single old tool result/live answered/cold replay read-only assertions. Settings retains the real F2 save path, same-alive and cold forced blocking, no-next-request observation after a real UI barrier, and canonical single settlement. No handler acknowledgement is fabricated.

The existing isolated ContentController/TestSandbox, temporary project, HOME/GROW_HOME and explicit removal of ask preference/timeout environment overrides remain in use. Provider messages, actual screens, decoded canonical Timeline, and referenced input blob hashes provide the evidence; provider history alone does not establish durable admission.

Unknown-ACK and persistence failure injection, child-to-primary routing, StoppedGoal boundaries, Plan regressions, and the separate original late blocking stall are not newly accepted by these PTY cases. The bounded mouse opening/query observation is recorded above; its full-chain failure and U6 remain open. Scoped whitespace checks emitted no diagnostics; the new-file `git diff --no-index --check` exit 1 is the content-difference status, not a runtime test failure.
