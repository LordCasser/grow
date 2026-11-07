# Mouse reply body — bounded PageDown probe

## Status and scope

Completed as a diagnostic observation only: the original 30-second reply-body barrier failed again; after single-clicking the visible Worked row and sending the first PageDown, the marker became visible, and it remained visible after the second PageDown. Provider/input/query counts stayed `(3, 6, 2)`. The unchanged original panic then ended the test with 0 passed / 1 failed, exit 101. This is not a passing mouse product-chain result and does not reclassify the initial failure as a fixture error.

Writes were limited to the existing timeout branch in `pager/tests/pty_e2e/advisory_questions.rs` and this record. No production, shared harness, public API, text-selection setting, other tests, prior logs/verification, real user state or CLI build was changed. Cargo and these test/document write scopes are released after this record.

The probe uses the visible `Worked for` text without hard-coding a duration, single-clicks that row using SGR mouse input, sends PageDown (`ESC [ 6 ~`) once and records a frame, then repeats PageDown once and records another frame. It does not send Reply/query, Submit, a bare letter navigation key, or a new session action. Before and after each PageDown it checks foreground provider request count, canonical input-event count and stable-id admission query count remain identical. Regardless of the resulting visibility, the original missing-body panic follows.

## Artifact identity

The existing focused U6 CLI was verified before running; no rebuild was performed:

- Binary: `/Users/lordcasser/workspace/projects/grow/target/debug/grow`.
- SHA-256: `9c6446f5f5a51f30cbd13ff0709e0914cec649adeb3a4d2eb8855a01e43a9f8e`.
- BLAKE3 from this actual PTY process: `f222d1fb36b518245dada0951840cb71d6b25ec5cc2e06609e7bc63da64a10a1`, matching the focused artifact.
- Bytes: `424532712`; mtime: `2026-11-06T16:18:47.807573+00:00`.
- Build provenance: the previously accepted focused U6 artifact. It does **not** include later sampling-types classification changes. Cargo may compile the PTY test's current dependency graph; that does not rebuild or upgrade the separately designated CLI.
- Identity record: `/tmp/grow-ask-mouse-nav-identity.json` at `2026-11-06T16:41:20.761253+00:00`.

Current HEAD is `8ab486a6e45870561b91dba872934a6e24a795f1`. The separately recorded current source diff SHA-256 is `a3cffef66f65619f9a9c2c94362a1ecf327ad6c1c5d51fa4a24f88436371b1bc` over Pager/Shell/Tools/sampling-types source, CLI, Cargo.lock and root Cargo.toml. That current dirty-source identity is not claimed as this older CLI's build identity.

## Command

One background command with timeout=0, from repository root; Rust `+1.93.1`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, PATH including `$HOME/.cargo/bin`, and command-local removal of CARGO_TARGET_DIR:

```sh
PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow \
GROW_PTY_CAST_DIR=/tmp/grow-ask-mouse-nav-probe-casts \
GROW_PTY_REPLY_MOUSE_PROBE=1 GROW_PTY_REPLY_NAV_PROBE=1 \
cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline advisory_questions::advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only -- --ignored --test-threads=1 --nocapture
```

Full log: `/tmp/grow-ask-mouse-nav-probe.log`. Independent artifacts: `/tmp/grow-ask-mouse-nav-probe-casts`. Existing failed and successful artifacts remain untouched. The normal keyboard and Settings cases are not rerun.

## Actual result and frames

The sole command compiled successfully and ran one ignored main test. Actual terminal result: **0 passed / 1 failed, exit 101**; 213.80 s command duration (including 2m 51s test/dependency compilation), 38.06 s test duration; completed `2026-11-06T16:45:44.049766+00:00`. The first failure remained `new human Turn response: timed out after 30s waiting for screen text "ADVISORY_REAL_REPLY_TURN_DONE"`. The diagnostic count assertions all passed; the final failure was the retained original panic.

| Checkpoint/action | Marker visible | Provider requests | Canonical input events | Stable-id admission queries |
| --- | --- | --- | --- | --- |
| Original 30-second body timeout | No | 3 | 6 | 2 |
| Single click Worked row, then first PageDown | Yes | 3 | 6 | 2 |
| Second PageDown | Yes | 3 | 6 | 2 |

There are two Submitted facts total: the original user prompt and one late reply. The total canonical input-event count remained six; navigation added none. Both admission queries already existed before the probe (initial edit query and automatic completion query); the mouse focus and PageDown actions added none.

The same-session source identity is `01a1b1bf-facd-7ee2-9e46-5274453756ac`; stable reply id is `question-reply:01a1b1bf-facd-7ee2-9e46-5274453756ac:call_pty_advisory_late_reply`. All three native Timeline snapshots contain the assistant marker at domain sequence 62. The real body existed durably before any probe navigation.

Preserved independent artifacts, each with actual screen, cast, foreground provider JSON, RPC/unified logs, native checksummed Timeline and referenced input blobs:

- `/tmp/grow-ask-mouse-nav-probe-casts/advisory_reply_body_timeout.*`
- `/tmp/grow-ask-mouse-nav-probe-casts/advisory_reply_nav_page_down_1.*`
- `/tmp/grow-ask-mouse-nav-probe-casts/advisory_reply_nav_page_down_2.*`

The timeout screen shows only the new reply prompt and Worked status. The first navigation screen shows the original prompt/card, original response, new reply prompt, and the highlighted `ADVISORY_REAL_REPLY_TURN_DONE` assistant body. The second screen retains the same visible content. No snapshot was taken between the focus click and first PageDown, so these results establish visibility after their combined sequence, not which action alone caused it.

## Interpretation and stopping point

The actual body becomes visible through normal transcript focus/paging without another model request, input event, admission query, Reply, Submit or session action. This supplies a real display prerequisite and narrows the parent investigation toward view/layout/follow/selection or cache invalidation. It does not prove a unique internal cause, establish that PageDown alone is sufficient, or prove the first hidden body was a fixture error. No production fix, new internal instrumentation or second run followed. The initial automatic-visibility failure remains recorded and the whole mouse main case remains failed.

## Cleanup and release

The one background command's actual terminal exit was retrieved and the full log read from the beginning. At `2026-11-06T16:46:29.630653+00:00`, no owned Cargo/PTY executable remained. The designated CLI SHA-256 stayed `9c6446f5f5a51f30cbd13ff0709e0914cec649adeb3a4d2eb8855a01e43a9f8e`, and the separately captured current source diff remained `a3cffef66f65619f9a9c2c94362a1ecf327ad6c1c5d51fa4a24f88436371b1bc`. Real user config/session/Goal/leader processes were not touched.

Cargo and `advisory_questions.rs`/this verification record write scopes are released. No default keyboard/Settings rerun, fresh CLI build, full suite, formatter, production edit, commit, push, archive or release command was performed. This result is a bounded diagnostic candidate for parent review, not whole-feature acceptance.

## Evidence boundaries

The existing ContentController/TestSandbox isolated HOME/GROW_HOME/project and loopback provider; the actual Pager process used the verified existing focused binary and the real mouse/question/input path. Native Timeline snapshots were decoded through the existing checksummed testkit. These observations concern that U6 focused artifact only, not the later sampling classification changes.
