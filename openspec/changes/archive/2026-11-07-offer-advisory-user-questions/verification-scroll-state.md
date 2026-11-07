# Mouse reply body: one bounded Scrollback state trace

## Result and ownership

诊断已完成，待父 review／形成修复契约。首次不绘制的直接原因已落到 **Scrollback layout 与 follow-preserve 的坐标锚定边界**：新 prompt 在精确布局中位于 y=11；布局重建把上方旧 card 从已测高度 2 换成估计高度 1，将同一 prompt 移到 y=10，但 preserve offset 仍为 11。由此启用的 sticky header 占 6 行，实际 content paint 从 y=17 开始；正文 entry id=8 的完整一行位于 `[16,17)`，被 paint window 排除。正文并未丢失、折叠或进入 paint 后不出字。

唯一实际 mouse＋nav run 保留原 30 秒 body barrier，实际 **0 passed / 1 failed，exit 101**。单击独立帧仍无正文；第一次 PageDown 后正文出现；provider/input/query 始终 `(3,6,2)`。生产行为未修复，mouse 产品验收仍失败。本轮没有以 generic double-click fold 定根因，也没有实施跳过 fold 或修改输入／结算／candidate owner 的方案。

临时 trace 已由精确 `search_replace` 全部撤回，生产文件逐字节等于任务开始基线。最终保留的测试差异仅为原 timeout probe 在单击后增加独立 checkpoint、可见性输出和不新增 provider/input/query 的断言；另有本文。Cargo、临时 trace、全部 source/test/docs write scope 在交付后释放。

## Evidence and exact entries

新 run 的隔离 session 为 `01a1b1cd-7264-7ad1-be3b-76c959e45950`，stable reply id 为 `question-reply:01a1b1cd-7264-7ad1-be3b-76c959e45950:call_pty_advisory_late_reply`。实际 Pager PID 为 60145。四个 timeout／navigation Timeline 均为 79 行；物理第 73 行、domain seq 62 含 assistant `ADVISORY_REAL_REPLY_TURN_DONE`，response admission request 为 `71daf228-a901-4dca-be95-d5ebeb0b22a6`、attempt 1。checkpoint 继续通过现有 checksummed decoder 读取 native Timeline；未制造中间 state。

下述源码 index 均为零基，entry id 为真实稳定 id：旧问答 card `idx=1/id=2`；新 prompt `idx=4/id=6`；新正文 `idx=5/id=8`；reply Worked 行 `idx=6/id=9`。所有正文帧 id=8 的 text 都精确为 `ADVISORY_REAL_REPLY_TURN_DONE`，mode=Expanded，raw=false，pinned=false，pending=false；首次有正文时 running=true，完成后为 false。正文 cached height 与 unfolded_height 都是 1，measured=true。

原始 trace 位于各 checkpoint 的 **`.rpc.log`**（现有 `GROW_LOG_FILE` tracing sink），不是 `.unified.jsonl`。后者保留原 canonical/runtime 诊断。下表行号均引用 `/tmp/grow-ask-scroll-state-casts/advisory_reply_nav_page_down_2.rpc.log`；四份快照的已有前缀行号相同。

| Checkpoint / source line | Reply body y / height / measured | Prompt y | Old card height / measured | Offset / follow / preserve | Visible / paint range | Actual content origin and body |
| --- | --- | --- | --- | --- | --- | --- |
| Last correct prompt-only frame, 414–420, 16:59:56.348Z | Body not yet present | 11 | 2 / true | 11 / true / true | `0..5` / `4..5` | No pinned header, content y-screen=3, scroll=11 |
| First body-bearing frame, 421–430, 16:59:56.369Z | 16 / 1 / true | 10 | 1 / false | 11 / true / true | `0..6` / `6..6` | Pinned prompt idx4; 6 header rows; scroll-for-content=17; empty paint and Buffer marker_rows=[] |
| Completed reply frame, 504–513, 16:59:56.595Z | 16 / 1 / true | 10 | 1 / false | 11 / true / true | `0..7` / `6..7` | Only Worked idx6 paints; marker_rows=[] |
| Original 30-second timeout, last paint 572–580, 16:59:59.614Z | 16 / 1 / true | 10 | 1 / false | 11 / true / true | `0..7` / `6..7` | Content rect `(2,9,116,34)`, scroll-for-content=17, visible_blocks=[6], marker_rows=[] |
| Single Worked click only, 581–589, 17:00:26.196Z | 16 / 1 / true | 10 | 1 / false | 11 / true / true | `0..7` / `6..7` | Same content rect/range; active becomes true; marker_rows=[] |
| First PageDown, 590–602, 17:00:26.332Z | 17 / 1 / true | 11 | 2 / true | 0 / true / true | `0..7` / `0..7` | Content rect `(2,3,116,40)`, scroll-for-content=0, Buffer row 20 contains same marker |
| Second PageDown, 603–611, 17:00:26.630Z | 17 / 1 / true | 11 | 2 / true | 0 / true / true | `0..7` / `0..7` | Same body and Buffer marker row 20 |

The timeout's last rendered frame predates the checkpoint: idle produced no later paint until the focus click. That click gives an independent fresh frame with the same failing geometry, ruling out reliance on an old screen snapshot.

At timeout/click, the old card remains `mode=Collapsed, pinned=true, running=false, pending=false, raw=false`, y=4, height/unfolded=1, measured=false; it is still collapsed after PageDown, when its exact height/unfolded becomes 2. Its content source does not change. All entries have group-header count=0, group-collapse=false, verb-group-header=false. `dirty={}`, `gaps_dirty=false`, batch=0 at the recorded paint windows. There is no field named `dirty_generation`; actual equivalents recorded are dirty-entry membership and state `generation` / `content_generation`.

State generations are 25/12 at the last correct prompt-only frame, 29/16 at the first body frame, 32/18 at timeout and click, 33/18 after first PageDown, 34/18 after second. `selected=Some(4), current_turn=Some(1), view_mode=AllTurns` persist from new-prompt pin through the focus click; PageDown selects idx5. No turn filter excludes the body: visible range always includes it. Actual search_filter=false and dim_from=None. The cached Turn table reports `[prompt0..4 Completed, prompt4..7 Running]` at timeout despite canonical completion; this field was observed, not changed or audited as another issue.

## First broken invariant and bounded explanation

The first observed mismatch is **a preserve pin outliving the coordinates that established it**, between the last prompt-only frame at line 420 and the first body frame at line 421. At line 421 the prompt's old estimated height 3 is measured to exact 5 while its y is already 10; the below-window body is measured at line 422. Old entries above the viewport remain estimates, including the card whose height changed from exact 2 to estimate 1. `scroll_offset` remains 11 throughout. The cached prefix itself is internally consistent: prompt y=10, height5+gap1, so body y=16; body height1+gap1, so Worked y=18. The stale relationship is between that prefix and the preserve pin, not an inconsistent prefix sum.

Relevant implementation boundaries, inspected without changes:

- `state/nav.rs::follow_new_turn` → `scroll_to_entry_top` / `entry_top_scroll_offset` measures the prompt target and arms follow-with-preserve. This run establishes offset=prompt y=11.
- `state/mod.rs::prepare_layout` Case 1 → `state/layout.rs::rebuild_layout_cache` replaces heights with estimates, then calls `handle_follow_mode` and `settle_visible_measurements`. Only width changes in the non-follow path capture/restore a scroll anchor. `measurement_window` deliberately has no above margin, so the old card remains estimated when the top is at the reply prompt.
- `state/nav.rs::follow_scroll_to_bottom` in preserve mode keeps the absolute offset when `max_offset <= offset < total_height`. In this run `max_offset=0`, offset11, total18 then20; neither overflow nor past-end healing applies. A page-flip pin being above max_offset is an intentional supported state; blindly calling that inequality the bug would miss the referent drift.
- `scrollback_pane.rs::render_with_sticky_headers` computes a pinned prompt after scroll11 passes prompt y10. `sticky.rs::header_screen_rows` includes its gap; `scroll_for_content(11)=11+6=17`. `render_content` passes this exact coordinate to `paint_window`.
- `state/layout.rs::compute_paint_window` excludes an entry ending exactly at the content start. For body `[16,17)` and viewport start17, `paint=6..7` correctly omits idx5 under the supplied geometry. The renderer never receives this body slice. Its Buffer trace confirms no marker, so changing body output cache/selected styling cannot explain this initial exclusion.
- `state/nav.rs::page_down` → `scroll_down` clamps offset to max0; `select_viewport_edge` settles the newly exposed entries, including card height1→2 at log line591. Prompt/body shift back to y11/y17, paint contains idx5, and the same body renders immediately.

These observations sufficiently identify the first incorrect owner as layout/follow-preserve anchor consistency. The exact caller that invalidated the cache between the two frames was not separately traced with a call stack. `remove_entry` invalidates it, and the existing preview-replacement path can call that method, but this run does **not** establish that caller as the unique trigger or assign a tracker/candidate defect. No further renderer or upstream audit followed.

Generic double-click fold is one real prior gesture, not the diagnosed cause: its actual collapsed card is already exactly measured at height2 and the prompt pin is correct afterward. The later full-cache estimate changes the prefix under an already armed pin. PageDown leaves that same card collapsed and reveals the same body. No fold semantics were changed to test a theory.

## Falsifiable repair criteria for the parent

The parent can hand one bounded repair to the Scrollback layout/follow owner: when a layout rebuild or lazy-height replacement changes rows before a preserve-pinned prompt that still exists, preserve the same prompt-relative content position using the current geometry. A correct result must prevent an estimate-only one-row displacement from enabling a sticky header that skips the reply's first body line. This is a behavioral criterion, not approval of a particular new field, helper, API or estimate special case.

Verification must retain the real no-Tab mouse main case, unchanged selection settings, one literal late reply, original 30-second body assertion and `(3,6,2)` counts. It must display the reply automatically before any Worked click/PageDown. A local regression may use the actual prompt/card/body block types and the full rebuild path, with the preceding card known to have estimate1/exact2; it should verify the same pin referent and body paint inclusion before and after settling. Constructed state can support that local invariant, but cannot replace the actual mouse acceptance run. Existing manual-fold/preserve behavior remains a separate invariant to retain. No repair or additional test run was performed here.

## Build, run and source identity

HEAD is `8ab486a6e45870561b91dba872934a6e24a795f1`, with the parent's existing shared dirty implementation retained. The fresh build includes the current source, including the later archived standard Messages classification changes; it is not the older focused U6 binary.

- Exact task-local instrumentation/probe patch: `/tmp/grow-ask-scroll-state-instrumentation.patch`, SHA-256 `184623763b1e2da2e1c21137de918dff7e1ebb635c6117f817048069562132c2`.
- Tracked production-source diff captured before build: `/tmp/grow-ask-scroll-state-production.diff`, SHA-256 `f3439b92e88a93bc8dc6da982c082dbcb49a40511857ec5ad910f7cd580f96a5`; covers Pager/Shell/Tools/Sampler/sampling-types/CLI source, Cargo.lock and root Cargo.toml. This is a dirty-diff identity, not a whole-checkout commit identity.
- Instrumented source metadata/hash/mtime: `/tmp/grow-ask-scroll-state-instrumented-source.json`; source-before byte baseline and metadata: `/tmp/grow-ask-scroll-state-source-before/` and `/tmp/grow-ask-scroll-state-source-before.json`.
- `scrollback_pane.rs` instrumented SHA-256 `1d8fe6a7073c43eb3e4041f63e8b52806be81e4c5a5d28652366b47dbcd8bf53`, mtime_ns `1793984128018469286`.
- `state/layout.rs` instrumented SHA-256 `546f1d911b7c2fad078b65f26a1b0cb2a77ac16751e1984d9fec42f42308e159`, mtime_ns `1793984155988349607`.
- Instrumented PTY file SHA-256 `aadde841d032930b183d533a52ae944974b5bc5470cc8a729e547408be96cf43`, mtime_ns `1793984243255976667`.
- Designated CLI `/Users/lordcasser/workspace/projects/grow/target/debug/grow`: 424570920 bytes; mtime_ns `1793984292753491846` (`2026-11-06T16:58:12.753491846Z`); SHA-256 `399b06b95db6276dd7822efbf3752b7844603de19cfa711f429a9ab6daa67879`; actual PTY BLAKE3 `53191cd3a61f28f9383d9ccd3a48c76aa09e7951a9eda4c090bb553c697caee1`.

Both commands ran from repository root with Rust `+1.93.1`, `--locked --offline`, PATH including `$HOME/.cargo/bin`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, and command-local removal of inherited CARGO_TARGET_DIR. Both used background=true, timeout=0, and their actual terminal exits were collected.

```sh
env -u CARGO_TARGET_DIR -u PAGER_BINARY PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 build -p cli --bin grow --locked --offline \
  > /tmp/grow-ask-scroll-state-build.log 2>&1

env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow \
  GROW_PTY_CAST_DIR=/tmp/grow-ask-scroll-state-casts \
  GROW_PTY_REPLY_MOUSE_PROBE=1 GROW_PTY_REPLY_NAV_PROBE=1 \
  GROW_PTY_SCROLL_STATE_MARKER=ADVISORY_REAL_REPLY_TURN_DONE \
  cargo +1.93.1 test -p pager --test pty_e2e_shell_tools --locked --offline \
  advisory_questions::advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only \
  -- --ignored --test-threads=1 --nocapture \
  > /tmp/grow-ask-scroll-state-product.log 2>&1
```

Build actual exit0, 93.75s. Product actual exit101, 86.11s; test compilation39.68s, one test41.08s, 18 filtered out. This run failed at the retained original body timeout panic, after all navigation count assertions passed. No default keyboard/Settings, workspace suite or repeat mouse run followed.

`openspec validate --all --strict --no-interactive` completed with actual exit0: **22 passed, 0 failed**; log `/tmp/grow-ask-scroll-state-openspec.log`. Final production scope has empty `git diff`. Scoped whitespace checks emitted no diagnostics; `git diff --no-index --check /dev/null` returned1 for each untracked test/document because they are new content. The final test differs from the executed instrumented test only by removing temporary log-target/env wiring; no second execution was made. Timeout and second-PageDown raw Timeline bytes are identical, independently confirming that navigation added no canonical facts.

## Artifacts, trace removal and remaining limits

Independent artifacts directory `/tmp/grow-ask-scroll-state-casts` preserves original-ended/editor/Other/submit checkpoints, then each of `advisory_reply_body_timeout.*`, `advisory_reply_nav_click.*`, `advisory_reply_nav_page_down_1.*`, `advisory_reply_nav_page_down_2.*`: screen, cast, provider requests, RPC trace, unified log, native Timeline and referenced immutable input blobs. Previous `/tmp/grow-ask-mouse-nav-probe*` and `/tmp/grow-ask-focused-*` evidence is untouched. Compact derived indices live at `/tmp/grow-ask-scroll-state-checkpoints.json` and `/tmp/grow-ask-scroll-state-transitions.json`; raw logs are authoritative.

Temporary tracing used only the existing tracing sink, gated by the task-local marker env and the exact sandbox prompt `ASK_ADVISORY_PTY`. It observed existing exact/dirty measurement results and actual paint-window/Buffer state. It did not compute extra heights or render outputs, modify timing/layout decisions, add public API/config/persistent fields, add a helper, or touch nav/selection source. The hermetic harness clears inheritance, so the temporary test-local marker pass-through and tracing target were explicitly installed for this run and then removed.

At `2026-11-06T17:04:22.694492Z`, the cleanup record `/tmp/grow-ask-scroll-state-cleanup.json` verifies all four assigned production files equal their original baseline bytes; nav/selection were never edited. Restored pane SHA-256 is `22b1453853a4f8556b23ab499264ccc3bdf129671eef45817844136d55f9a217`; restored layout SHA-256 is `f7423ccf0158010735a2d3cc0154ca2f57b273bcfcf9268b8312ec542eacd225`. Final test SHA-256 is `d863d85e6a490cbe4d63d5b221b88b090af3665d7452daa4d80be81a6c4d3f3d`; the retained four-line test diff is `/tmp/grow-ask-scroll-state-final-test.patch`.

No owned Cargo, PTY test executable or actual Pager PID60145 remained at cleanup. The binary hash remains the instrumented identity above: **current source has no trace, while target/debug/grow still contains the temporary trace code** and represents this diagnostic build. It was not rebuilt after trace removal; subsequent product acceptance must deliberately build the owner's final source rather than treat this binary as an uninstrumented final artifact.

The observed failure and first broken anchor invariant are established for this isolated real mouse run. The invalidation caller's precise runtime call stack, a selected implementation for anchor preservation, and final repair acceptance remain parent-owned. There is no production fix, acceptance/archive, commit/push, package/release action or real-user-state mutation in this delivery.
