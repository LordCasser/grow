# U7 final product acceptance: fresh CLI, mouse and controls

## Result and scope

The U7 product acceptance passed on the first execution of all three requested cases using one freshly built, uninstrumented CLI: **mouse main 1 passed / 0 failed, exit 0; natural keyboard main plus Settings 2 passed / 0 failed, exit 0**. The original mouse reply body appeared automatically within the unchanged 30-second barrier. No Tab, PageDown, Worked click, extra navigation or retry was used to make it appear. The timeout navigation probe was unset and never executed.

This executor changed only this record. Production source and the existing PTY test were not edited; no fixture correction or new instrumentation was needed. [U7 implementation verification](verification-follow-pin.md) and [parent design/review](review-follow-pin.md) remain the implementation authorities. Earlier mouse failures and diagnostic artifacts remain preserved. These three product cases complete this bounded acceptance assignment; parent review, final diff integration, milestone acceptance and OpenSpec archive remain parent-owned.

## Artifact and source identity

The build used HEAD `8ab486a6e45870561b91dba872934a6e24a795f1` with the accepted U7 state changes and other existing shared dirty implementation. Before building, the executor rechecked root AGENTS and the software-engineering skill, read the current U7 handoff and test entry, and found no nested AGENTS or prior temporary scroll-state trace in Pager source. Current source includes the previously archived standard Messages classification changes. The earlier instrumented `target/debug/grow` was replaced by this fresh build; its old identity is not used as acceptance evidence.

| Identity | Actual value |
| --- | --- |
| CLI | `/Users/lordcasser/workspace/projects/grow/target/debug/grow` |
| SHA-256 | `a01463242b2a161cd43a4133bbeadcc92338ff3df317e108c6a0bdb4effc8bb3` |
| Actual PTY BLAKE3, identical in all three cases | `bd59f0769fca56448a83996e88a52e99b9a381cd535b0a9e15e111205252f205` |
| Bytes | `424505832` |
| Binary mtime | `2026-11-06T17:40:12.306856836Z`, mtime_ns `1793986812306856836` |
| Tracked dirty diff SHA-256 | `b723954e66f5ac838ebca1ac17864916755ea18b12111bf2628df3b14e12255f` |
| Rust source/manifest inventory SHA-256, 2163 files | `55e83d7dd11c387b5a27a690b6474f4607dc0df1aa25060f414eb4964ba96289` |
| Unchanged PTY test SHA-256 | `d863d85e6a490cbe4d63d5b221b88b090af3665d7452daa4d80be81a6c4d3f3d` |

The dirty diff covers tracked `crates/`, Cargo.toml, Cargo.lock, rust-toolchain.toml and `.cargo/`; it is saved at `/tmp/grow-ask-final-u7-source.diff`. The inventory includes tracked/untracked Rust files and Cargo manifests under `crates/`, plus root toolchain/Cargo configuration; its hash is over sorted path/content-digest rows. Full per-file hashes, sizes and mtime are in `/tmp/grow-ask-final-u7-source-before.json`. CLI metadata is in `/tmp/grow-ask-final-u7-binary-identity.json`.

All 2163 inventory files retained their content **and mtime** through build and both test commands. The binary SHA-256 and mtime also remained unchanged after testing, independently confirming that all cases used the same fresh artifact. No temporary trace code or env pass-through was added.

## Actual commands and terminal results

All commands ran from repository root with Rust `+1.93.1`, `--locked --offline`, `PATH="$HOME/.cargo/bin:$PATH"`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, and command-local removal of inherited CARGO_TARGET_DIR. Each long command used background=true, timeout=0. Python subprocess wrappers retained exact start/end/exit records; every actual terminal result was retrieved.

| Command | Actual result | UTC start → completion | Duration | Log |
| --- | --- | --- | --- | --- |
| Fresh CLI build | exit 0 | 17:39:43.843945 → 17:40:12.533941 | 28.69 s | `/tmp/grow-ask-final-u7-build.log` |
| Mouse exact main | 1 passed / 0 failed, exit 0; 18 filtered | 17:40:52.328804 → 17:41:59.594892 | 67.27 s command; 14.06 s test; 48.44 s test compilation | `/tmp/grow-ask-final-u7-mouse.log` |
| Natural keyboard main + Settings | 2 passed / 0 failed, exit 0; 17 filtered | 17:42:30.781809 → 17:42:56.691515 | 25.91 s command; 24.46 s tests | `/tmp/grow-ask-final-u7-controls.log` |

The commands, omitting only the timing/JSON wrapper, were:

```sh
env -u CARGO_TARGET_DIR -u PAGER_BINARY -u GROW_PTY_REPLY_MOUSE_PROBE \
  -u GROW_PTY_REPLY_NAV_PROBE -u GROW_PTY_SCROLL_STATE_MARKER \
  -u GROW_SCROLL_LOG -u RUST_LOG PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 build -p cli --bin grow --locked --offline

env -u CARGO_TARGET_DIR -u GROW_PTY_REPLY_NAV_PROBE \
  -u GROW_PTY_SCROLL_STATE_MARKER -u GROW_SCROLL_LOG -u RUST_LOG \
  PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow \
  GROW_PTY_CAST_DIR=/tmp/grow-ask-final-u7-mouse-casts \
  GROW_PTY_REPLY_MOUSE_PROBE=1 \
  cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline \
  advisory_questions::advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only \
  -- --ignored --test-threads=1 --nocapture

env -u CARGO_TARGET_DIR -u GROW_PTY_REPLY_MOUSE_PROBE \
  -u GROW_PTY_REPLY_NAV_PROBE -u GROW_PTY_SCROLL_STATE_MARKER \
  -u GROW_SCROLL_LOG -u RUST_LOG PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow \
  GROW_PTY_CAST_DIR=/tmp/grow-ask-final-u7-controls-casts \
  cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline \
  advisory_questions:: -- --ignored --test-threads=1 --nocapture
```

No trace variables were inherited at preparation. Both PTY wrappers additionally removed any `GROW_`/`PAGER_` variable containing `TRACE`; their result records confirm nav=null and trace_env_names=[]; controls also confirms mouse=null. Hermetic ContentController launches clear parent inheritance and install only existing sandbox environment operations, including the ordinary `acp_transport::gateway=debug` artifact log. Result records are `/tmp/grow-ask-final-u7-{build,mouse,controls}-result.json`.

## Product-chain evidence

Both mouse and natural-keyboard mains exercised the complete existing real chain: mock provider calls `ask_user_question` with blocking=false; Pager visibly presents exact Agent recommendations; Shell proceeds to the next provider request without a human answer; the old tool result explicitly says the user has NOT answered. The normal composer remains editable while the original Turn runs, and its draft survives the local editor. The original Turn durably ends before Reply opens.

Mouse uses the existing double-click on the visible Reply body with unchanged text-selection settings; keyboard uses the visible `r` action. Both then immediately accept Down/Enter in the real QuestionView, select `/quit`, advance to the second question, edit Other notes with literal `/new LATE_REPLY_NOTES_LITERAL`, and submit. There is no editor-focus Tab. The new provider request retains question/source identity, Agent recommendation provenance, real human selection and slash-style notes as data. The immutable input payload is checked against canonical BLAKE3, has user origin and FIFO route, and has no Skill invocation. One distinct new user Turn is admitted after the original Turn end.

| Evidence checkpoint | Mouse provider/input/query | Keyboard provider/input/query | Verified state |
| --- | --- | --- | --- |
| Original Turn ended | 2 / 3 / 0 | 2 / 3 / 0 | One old tool completion and one model-visible result |
| Reply body admitted | 3 / 6 / 2 | 3 / 6 / 2 | Automatic visible body; canonical assistant domain seq62 |
| Live Answered | 3 / 6 / 2 | 3 / 6 / 2 | Visible `Answered — read-only`, no extra Reply query |
| Cold Answered | 3 / 6 / 3 | 3 / 6 / 3 | Same reply/input/session, read-only; one allowed cold canonical query |

Input-event totals include Submitted, admission-resolved and Consumed for the initial prompt and one late reply: two of each, six total. Query totals are cumulative in the same sandbox RPC file across live and cold processes; they are not counts of accepted inputs. Old-tool results per foreground request are `[0,1,1]`; canonical old-tool completion and model-visible tool-result counts stay exactly one through cold recovery. No extra provider request, duplicate input or new session was produced. The read-only double-click does not reopen an editor, and ordinary composer input remains usable.

The original body screen barrier passed before `show_original_card_from_prompt` runs its existing history-return gestures. No `advisory_reply_body_timeout` or `advisory_reply_nav*` artifacts exist in either new directory. The main casts first emit the complete marker at **4.776160333 s (mouse)** and **2.088151833 s (keyboard)** from process recording start; these are recording timestamps, not submission latency benchmarks. The admitted screens already show the body before history navigation.

Mouse source session: `01a1b1f3-e488-7170-8944-dad11fac7be8`; input `input-e4035c1721573845ee2cb24cabe065f2460a0c35c70612d5bd79e3d63f46dbb3`. Keyboard source session: `01a1b1f4-90d8-72b1-a7c1-bb35d1479827`; input `input-f02a2b4fe190c745b40ee907fa579dec506b6ccbed0dc909627f4429906967ec`. Each stable reply id is `question-reply:<source>:call_pty_advisory_late_reply`. Derived count/timestamp indices are in `/tmp/grow-ask-final-u7-product-evidence.json`; raw artifacts remain authoritative.

## Settings control

The same-artifact Settings case passed using actual F2 Settings search, selection and save. It asserts persisted `toolset.ask_user_question.allow_non_blocking=false` while retaining timeout_enabled=false, timeout_secs=71 and the unrelated seeded raw field. It returns to the same live session, then sends an ask whose model argument is still blocking=false. The actual blocking QuestionView is the causal barrier; the test verifies no continuation during its existing 900 ms observation window, then changes to `Human choice` with Down/Enter and observes the provider continuation only after that answer.

A clean process exit and cold `--continue` retain the saved false preference and the same session. The second ask follows the same forced-blocking behavior. Each call has exactly one canonical completion and tool result. Provider counts follow the existing assertions: one setup request, live ask 1→2 and answer continuation 2→3, cold ask 3→4 and answer continuation 4→5. The test does not invoke a settings setter directly. Actual passed assertions establish saved TOML and Timeline behavior; the existing Settings artifact is `/tmp/grow-ask-final-u7-controls-casts/advisory_settings_continue.cast`, which preserves the cold interaction. No new Settings checkpoint facility was added.

## Retained artifacts, cleanup and limits

Independent mouse and controls directories are `/tmp/grow-ask-final-u7-mouse-casts` and `/tmp/grow-ask-final-u7-controls-casts`. Each main retains original-ended/editor/Other/submitted/admitted/live-Answered/cold-Answered checkpoints, including actual screen, cast, provider JSON, RPC/unified logs, native Timeline and referenced immutable input blobs, plus full `advisory_reply_main.cast` and `advisory_reply_continue.cast`. Existing focused, navigation and scroll-state artifacts were not overwritten.

At `2026-11-06T17:44:01.591916Z`, `/tmp/grow-ask-final-u7-cleanup.json` verifies unchanged source contents/mtime and binary identity, and no remaining Cargo, PTY executable or CLI process. The observed advisory Pager PIDs 68662, 68678, 68725 and 68740 were retired; successful `quit_cleanly` checks also require the Settings processes to exit0 and drain. All three background commands have actual terminal results. No watch was started and none remains owned by this task.

Final document whitespace review emitted no diagnostics (`git diff --no-index --check` returned1 because this is a new untracked file). `openspec validate --all --strict --no-interactive` completed with actual exit0, **22 passed / 0 failed**; full log `/tmp/grow-ask-final-u7-openspec.log`. No tasks were marked for the parent and no archive command was run.

Platform: macOS 27.0.1, arm64, existing Unix PTY harness with isolated HOME/GROW_HOME/project, loopback mock provider and `--no-leader --trust`. No real-user config, session, Goal, API or leader was touched. This round did not rerun F3, the workspace suite, fault/ACK injection, child-to-primary routing, Goal boundaries, Plan regressions or other-platform terminal behavior. No formatter, production fix, commit/push, package/publish, archive or release action was performed. Cargo and all assigned PTY test/document write scopes are released on delivery for the parent's final integration review.
