# Verification — TUI source scans and copies

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

## Outcome

The optimization reduces unnecessary image detection work for image-free long messages while preserving the existing replay, layout and persistence boundaries. The diagnostics-off 2048-turn ordinary PTY last-history median falls from **552.8 to 428.6 ms (22.5%)**; slow output falls from **668.5 to 565.8 ms (15.4%)**. No general first-frame improvement is claimed. Small sessions, image-heavy text and slash-dense candidates do not show consistent speedups. Readiness and full history delivery remain coupled.

## Build and comparison boundaries

- macOS arm64; actual Homebrew `rustc/cargo 1.99.1`, not the repository-pinned 1.93.1. See build context. Release, default CLI features, locked/offline dependency resolution, 4 build jobs, incremental disabled. Shell test support is enabled only for its test/benchmark builds. No release compiler flags were relaxed.
- Both binaries include the same optional first-send/applied/frame diagnostics. CPU differences are limited to borrowing normalized source, conservative image scan guards, one dimension probe for the same bytes, and allocation-free ASCII Mermaid candidate matching. No new dependency, storage/index/cache, batch-size, frame-cadence, ready, ACK or notification-order change.
- All 2,047 frozen source/manifest hashes were rechecked after measurement. The six changed files comprise three production files and three test files. Source snapshots include shared earlier work; this change is not a diff against Git HEAD.
- Shell and `session_load_perf` before/after binaries are byte-identical. Backend measurements are a negative control, not a backend optimization result. The final before CLI was rebuilt after common marker code was frozen.
- No compiler was observed in formal-start/end process snapshots and sampled checks. Warmups for both binaries and all PTY scales are excluded from medians. Each measured group runs serially in A/B, B/A, A/B order, n=3 per stage/scale; diagnostics-off and detailed-log groups are separate. Results are local measurements, not a statistical significance or hardware-independent promise.
- An initial before baseline contained a slow first 128-turn sample (~974ms). Its cause was not isolated; it is not mixed into the paired medians or compared against a warmed after binary. Compilation-time diagnostic smoke is excluded from performance data.

## End-to-end release results

All values below are milliseconds, median [min, max], n=3 per cell. `history_visible_ms` is the final historical marker observed by the PTY screen oracle; `ready_visible_ms` is the ready prompt observation. The separate stable-prompt hold includes 500ms and is not treated as resume latency. With instrumentation off, the internal `ready_ms` is deliberately unavailable.

| Fixture | Last history before | Last history after | Ready prompt before | Ready prompt after |
| --- | ---: | ---: | ---: | ---: |
| modern 128 | 133.9 [131.4, 148.2] | 142.1 [135.8, 164.2] | 134.0 [131.5, 148.2] | 142.2 [135.8, 181.2] |
| modern 512 | 173.2 [172.6, 184.6] | 174.0 [172.6, 175.9] | 217.9 [216.1, 291.3] | 201.9 [187.4, 212.7] |
| modern 2048 | 552.8 [552.2, 554.8] | 428.6 [426.4, 435.8] | 589.3 [583.9, 600.0] | 462.8 [460.3, 471.6] |
| slow 512 | 299.1 [298.1, 304.4] | 289.8 [283.6, 290.0] | 322.9 [322.9, 323.6] | 320.2 [283.7, 324.3] |
| slow 2048 | 668.5 [660.4, 690.1] | 565.8 [535.0, 598.7] | 668.6 [660.5, 690.1] | 588.1 [586.1, 638.7] |

Ordinary 512-turn last-screen medians are essentially unchanged (173.2→174.0ms); 128-turn medians rise (133.9→142.1ms) with overlapping ranges. Do not claim universal end-to-end acceleration. Detailed logs corroborate the 2048-turn last-screen reduction: ordinary 555.5→449.5ms, slow 666.2→551.7ms. Diagnostic logs are not substituted for the default-off result.

The default-off results below are the final performance evidence; detailed attribution was used only to explain phases. Slow-writer fixtures keep their original progress assertions and pass on both binaries; loading input suppression, Ctrl+D/re-resume and ready-key echo assertions pass. No thresholds were weakened.

## Isolated costs and non-benefits

The no-draw semantic fixture recovers and projects the modern Timeline, then consumes every notification and validates all message bodies/order. `decode_project_ms` is storage recovery plus projection, **not frontend wire JSON decode**. Each assistant body is ~32KiB; 2048 turns contain 67,141,632 assistant source bytes and 4,096 notifications. PTY adds its end sentinel and is a different fixture; do not subtract RPC/no-draw absolute times from PTY.

| Turns | Handler before | Handler after | Decode/project before | Decode/project after |
| --- | ---: | ---: | ---: | ---: |
| 128 | 9.5 [9.3, 9.5] | 4.0 [4.0, 4.1] | 14.8 [14.1, 14.9] | 14.3 [14.3, 15.8] |
| 512 | 36.7 [36.6, 36.7] | 15.3 [15.2, 15.9] | 57.3 [56.5, 57.3] | 56.2 [56.1, 57.1] |
| 2048 | 147.3 [146.2, 152.3] | 62.6 [60.3, 62.6] | 234.0 [223.6, 241.3] | 230.5 [225.8, 235.8] |

2048-turn handler CPU drops **57.5%**. In the instrumented ordinary PTY, per-run image-reference timer sums drop 90.138→6.061ms, message finish 112.993→23.589ms and apply-batch 182.861→89.226ms. Timeline read/decode remains 203.817→202.962ms and history projection 11.250→10.919ms. Frame-render totals remain 55.577→57.386ms. These stages overlap/nest and cannot be added into a wall-clock saving.

The microbenchmark times only deferred push plus completion (512 messages); fixture setup, copy/render oracles and assertions run outside its timer. All source bytes are still stored/copied by the ordinary append path; this is not a zero-copy replay. Values are ms, n=3.

| Source fixture | Before | After | Interpretation |
| --- | ---: | ---: | --- |
| plain_32k | 36.5 [36.4, 36.6] | 15.5 [15.1, 16.4] | 57.5% reduction; negative scans avoided |
| slash_code_dense | 39.3 [38.9, 40.3] | 40.2 [39.6, 40.3] | no gain; bare-path regex remains necessary |
| image_markdown_repeated | 196.3 [195.0, 203.4] | 196.8 [196.4, 198.0] | no gain; 512 refs to one real PNG, dedup retained |
| unicode_false_markers | 360.3 [360.0, 376.5] | 345.1 [342.2, 357.1] | small reduction; conservative Mermaid parse retained |

Micro data, semantic data. Slash/image candidates still pay their regex/parse/file costs; the tiny dimension-probe reduction does not make these aggregate cases materially faster. No representative mixed-media performance claim is made.

The byte-identical RPC control preserves complete-history/body/order oracles (128/2048 turns, n=3). At 2048 turns first history is 227.020→226.760ms, last history 377.201→370.058ms and load RPC 392.221→384.651ms. Small differences on identical binaries are run variance, not a claimed optimization. See RPC results.

## First-history attribution

Opt-in markers use the same session/cursor and writer sequence:

- `session.replay.first_send`: first post-coalescing `forward_attempt`, before gateway enqueue. Completion API does not expose accepted state; this event alone never proves delivery.
- `pager.replay.first_applied`: first actually applied history cursor in the active loading window. Background/direct branches reuse the already parsed notification.
- `pager.replay.first_frame`: first produced ordinary-root frame following history application with a new writer sequence and unobscured nonempty scrollback. It may contain later history. It does not prove the source cursor row is on screen.
- Existing successful `pager.frame.delivery` and `frame_ack` are matched by sequence; physical PTY marker observations remain independent. Child fullscreen/Dashboard/obscured views are outside the root-frame metric, not counted as false successful frames.

All **30** nonempty formal root replay pipelines have exactly one send/applied/frame marker; session/cursor agree and sequence maps to one successful delivery and one ACK. Correlation data, summary, analyzer. Times below share instrumentation UTC and are relative to the inferred start of `session.load_session`; they are not relative to PTY spawn.

| 2048-turn metric | Ordinary before→after | Slow writer before→after |
| --- | ---: | ---: |
| Send attempt | 227.9→225.9 | 229.2→225.3 |
| First application | 228.0→226.1 | 229.4→225.4 |
| First frame produced | 359.6→379.5 | 276.3→284.6 |
| First frame delivered | 359.7→379.6 | 324.9→334.7 |
| First present work | 30.2→56.3 | 12.7→22.2 |

The unchanged pre-send boundary (~226–228ms) is expected: this change optimizes TUI consumption after recovery. First application follows send by only ~0.1–0.2ms in these fixtures. The ordinary first produced frame becomes **~20ms later**, even though last-history output is ~106ms earlier in the detailed run. First present starts somewhat earlier, but work grows 30.2→56.3ms. The captured first visible sequence is 1120/1136/1168 before, versus 2048 after: faster consumption changes which history is already pending at the unchanged frame boundary. This observation supports batching/scheduling and larger first-frame work as the explanation; it is not proof that a particular cursor’s source row was drawn. This change intentionally does not alter draw cadence.

Slow-writer first delivery also does not improve (324.9→334.7ms), while final output improves. Writer wait remains substantial (per-run total median 241.3→199.2ms). Remaining whole-history storage decode and frame/output scheduling put a floor under first-screen/slow-output latency. A later change would need a separate measured scope; none is implemented here. No readiness/history pagination split is introduced.

## Correctness and adversarial coverage

Scoped release results: **2,703 passed, 0 failed, 8 ignored** across 16 selections: pager 1,818; pager-render 174; Shell 711. Ignored entries are not counted as passed. Existing release-only exclusion `scrollback::state::tests::insert_block_before_rejects_an_already_committed_anchor` requires debug assertions, as in earlier changes; the assertion/test was not modified. This is focused affected-scope validation, not a claim of the entire workspace suite.

- ACP/meta/tracker/load/fork/event-loop/subagent suites cover ownership, cursor/reconnect, failure/retry, ignored loading input and replay lifecycle. Shell storage/history/persistence/runtime handoff/subagent scopes cover durable resume and child recovery; persistence source remains unchanged in the paired delta.
- Existing physical-prefix presentation oracle covers 44 updates across 7 ACP and 31 distinct Grow presentation families (37 Grow occurrences). The broader 46-fixture raw protocol golden has 33 Grow families; sampling-attempt/rewind-marker have separate handling tests. These are not all represented in one PTY mixed-media performance fixture.
- New source-scan oracles exercise empty/Unicode/false candidates, mixed-case and entity-encoded Mermaid, raw/pretty copy, copy-first/render-first, late append, and normalized source. Existing media/layout oracles compare eager/deferred rows, anchors, source selection, scroll info and independently calculated heights. Image widths 32/64/80/100/120, Mermaid 24/48/80/88/120, sticky-layout width sequence 28→52→23→80→120→80→28; root/child parity at 80/120.
- Image tests cover relative Markdown paths, Unix bare paths, duplicate/alt precedence, missing/corrupt files, formats/dimensions and separator/Unicode false candidates. Backslash drive/UNC strings are tested using readable Unix filenames, not claimed as native Windows filesystem execution. Same-byte dimension probing retains completion-time file observation; no cross-message file cache exists.
- Four manual release PTY checks pass: cold root/child/grandchild Ctrl+D resume, partial-leaf release without checkpoint replay, unique concurrent cold claim, and post-resume follow-up with history. No duplicate work/result observed by these oracles.
- Baseline added tests passed on old code (7 source tests, 9 image tests; ignored manual benchmark separate). Final format and whitespace checks pass. No test threshold, ready gate, history order or persistence check was removed to obtain performance.

[Adversarial review](adversarial-review.md) records candidate correctness, borrow lifetime, file observation, diagnostics scope and limits. Independent read-only source/data review findings are recorded there after final review.

## Completion

Strict active/spec and archive validations, final source/binary integrity and dedicated build cleanup completed. Only `target/tui-scan-release` is removed; shared target and live session data are retained. The removed binaries and scripts are not part of the retained result.
