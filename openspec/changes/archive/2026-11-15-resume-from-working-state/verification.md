# Verification

## Implemented increment

The resident suspension path reads the current Turn and outstanding interaction evidence from ChatState. It checks prior dispatch by exact call ID. Child suspension queries the resident root owner for delegation evidence. Tree and cold child reconciliation reuse indexed lifecycle facts from the validated actor and avoid re-folding the parent journal just to obtain child facts. Runtime and Control writes do not clone the cumulative lifecycle fold. The common Hook settlement path uses an open-Hook index, and compaction settlement prepares its terminal without cloning the Timeline.

## A/B at the Shell boundary

Both arms ran `agent::mvp_agent::runtime_handoff::tests::complex_history_exit_through_acp` on macOS, in the locked Cargo test profile with stack size 16 MiB. A used the working tree before this increment, with the two earlier Grow fixes present. B used the final increment including the single coherent ChatState query after review. Each trial generated a separate isolated home with the same fixture generator, arguments, and 16 KiB result payload. Session IDs and timestamps differed. Runs were sequential, three per arm and size. The exit endpoint is the ACP suspension acknowledgement with writer retirement, not terminal-process EOF. The load endpoint is the Shell session/load response, not Pager input or paint readiness.

| Timeline records (domain) | Timeline bytes | Shell load A → B median, ms | Exit confirmation A → B median, ms |
| ---: | ---: | ---: | ---: |
| 478 | 10,104,230 | 1,629 → 1,070 | 109 → 108 |
| 1,342 | 39,338,912 | 5,741 → 3,994 | 116 → 127 |
| 4,798 | 156,306,582 | 22,713 → 15,666 | 132 → 117 |

Load ranges were 1,628–1,643 vs. 1,043–1,073 ms, 5,666–5,809 vs. 3,991–4,130 ms, and 22,564–23,054 vs. 15,221–16,082 ms. Exit ranges were 109–152 vs. 107–108 ms, 115–138 vs. 126–206 ms, and 128–134 vs. 112–122 ms. All 18 runs passed the durable prefix and writer-retirement assertions. A preceded B. These are same-profile local elapsed-time samples, not an alternating randomized experiment. An earlier candidate sweep overlapped compilation and is excluded from this table.

The fixture scales tool results and active Surface along with history. It identifies the cost reduction of this increment, but cannot establish fixed-working-state scaling. Shell still reads and validates the complete cold journal and projects all display records. The 15.7-second largest-case result is consistent with remaining history-dependent work. Exit samples do not establish a consistent latency improvement. Terminal input-ready and first-page timings, bytes actually read and peak RSS were not measured.

## Correctness

- Shell targeted binary tests with `RUST_MIN_STACK=16777216`: 38 passed, 5 ignored. Coverage includes child retirement races, partial tree retry, lost ACK, in-flight compaction, open waits, planning completion, and real two-process restore of original Steer ownership. The new partial-tail case exits after durable suspension with a partial JSONL record and verifies recovery consumes the original input once and removes the partial tail. It covers the current journal path, not a working-state publication protocol.
- The final ChatState test binary built by `cargo test --locked -p shell -p chat-state --lib --no-run` passed 653 tests, with 3 ignored. The new test fixes model-visible context and the active wait while varying closed Turns from 1 to 100 to 1,000. It checks the returned suspension evidence at the real ChatState handle boundary.
- `openspec validate --all --strict --no-interactive` and `git diff --check` passed. The delta remains active because the proposed storage and Pager contracts are incomplete.

## Fixed-state terminal A/B

This run used `pager-pty-harness::fixed_surface_resume_size_curve_acceptance` on real macOS PTYs, the same 50×120 terminal, isolated homes, the same provider stub, and the same dev Cargo profile with `--no-default-features --features sandbox-enforce`. The default build hit a jemalloc configure error on the local SDK. A was built from `509c6d0ab` in a detached worktree. B includes the current-Turn fold, Pager paging, the exit-gate change, and the resident prompt-history read. Neither arm issued inference. A and B ran in alternating order, three pairs per size. The separate Fullscreen smoke runner measured direct process exit without the harness's terminal drain window.

The fixture held the model Surface at one system item and one closed user/assistant pair. Only committed 16 KiB UI observations before that pair increased. The PTY observed the newest marker in a completed frame, the exact typed confirmation, and exit status zero. The Ctrl+D column includes time from the first double Ctrl+D through retries of the baseline's startup gate to process exit. The test retried only that known startup result, with no force exit.

| Closed observations | Journal bytes | First paint A → B (ms) | Input echo A → B (ms) | Ctrl+D to exit A → B (ms) |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 20,900 | 216 → 210 | 228 → 220 | 116 → 118 |
| 512 | 8,614,301 | 738 → 633 | 763 → 655 | 884 → 219 |
| 2,048 | 34,446,274 | 2,302 → 2,456 | 2,373 → 2,561 | 1,995 → 220 |

The baseline required two retries in every 512-observation run and four in every 2,048-observation run. B needed zero retries at every size. The independent direct-exit sweep measured 119 ms for B at all three sizes. The harness exit column includes draining terminal output after observing process exit. First paint and input still scaled with closed history and were slower at the largest size in the harness run. This fixture isolates presentation growth. The earlier Shell experiment also exercised tool-bearing domain history. Neither establishes the required cold-resume bound.

A separate 512-observation B PTY run used Tab and PageUp to open an older page. It observed an older marker in the passive view, returned with Escape, echoed typed text in the composer and exited on the first Ctrl+D confirmation. This checks the terminal path through the extension and renderer.

The final run passed 653 ChatState tests (3 ignored), 10 focused Shell tests and 7 focused Pager tests. Coverage includes the torn-tail Steer recovery, page cut mutation and append, tool grouping, isolation from live controls, complete transcript fallback and exit while prompt history is loading. The checkpoint publication crash cases still require a persisted working-state protocol. The change remains active.

## Disk index and publication recovery

The local Pager writer publishes a generation-specific file of physical record references and sorted notice identity entries. It syncs the journal and index before replacing the manifest. A subsequent writer hydrates the retained prefix and validates complete records after the cut. The page reader checks the original bytes of each selected standalone notice. This cut leaves domain events and presentation facts with special dependencies in the existing fold.

`process_exit_around_index_publication_recovers_exact_tail` terminates a real subprocess after the index sync, before or after the manifest publication. Recovery verifies the expected generation, applies a complete domain tail once, ignores a torn final fragment, and durably appends the next sequence. It checks the event-counter floor, exact retry and conflicting payload, and verifies that changing an old notice with a valid journal checksum only fails when that record's page is read. The nine targeted Shell tests also cover reference bounds, cut replacement, grouping, writer-prefix reuse and the existing Steer crash recovery.

## 394 MiB terminal A/B

Both builds used the local dev profile with `--no-default-features --features sandbox-enforce`. A is the branch implementation immediately before disk indexing, including the earlier Pager and suspension changes. B adds the generation and presentation identity index. A separate Python runner launched the actual `grow --resume <id> --fullscreen` processes in 50×120 macOS PTYs, on isolated homes and the same Mock route. There were three alternating A/B pairs at each size and two process launches per session. Each session kept the same model Surface and fixed user/assistant pair while varying only closed 16 KiB notices with `eventId` and `agentTimestampMs`. No inference was submitted. The fixture and terminal observations are synthetic. The run does not control the OS page cache.

The table reports medians in milliseconds for the second process launch against the same session. First screen requires the newest marker in a complete terminal frame. Input requires composer echo. Exit measures double Ctrl+D through process exit with status zero. The old-page measurement includes switching focus and paging to the first rendered older notice. The small fixture fits in one page.

| Closed notices | Journal bytes | First screen A → B | Input A → B | Ctrl+D A → B | B older page |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 64 | 1,079,792 | 185 → 190 | 250 → 250 | 119 → 119 | — |
| 8,192 | 137,830,496 | 6,680 → 287 | 6,764 → 315 | 123 → 119 | 873 |
| 24,576 | 413,535,034 | 20,364 → 294 | 20,790 → 321 | 269 → 122 | 881 |

B's repeat-input ranges were 249–260, 313–334 and 319–454 ms. Its exit ranges were 119–120, 119–221 and 122–220 ms. Neither arm needed exit retries. First-open B input medians were 348, 8,785 and 25,542 ms. At the largest point first open still spent 24,944–26,992 ms before input. B's first open includes full index migration. The measured runs used small append tails and did not measure index advancement, bytes actually read or peak RSS. The in-tree Rust harness was compiled with the same size points and independently checks the folded Surface hash.

## Domain counterexample and remaining acceptance

One separate two-size PTY probe replaced the notices with closed domain observations of the same payload size, keeping the model Surface fixed. On the second process launch B took 270 ms to echo input at 1,068,547 bytes and 25,326 ms at 409,147,076 bytes. Ctrl+D was 221 and 232 ms. The domain probe overlapped the end of the presentation experiment and is a counterexample rather than a paired latency distribution.

Cold resume still rebuilds the domain fold and verifies historical prompt artifacts. It needs a durable executable working-state projection, disk lookups for closed domain identity and source references, and advancement of the published frontier. Pager still opens earlier pages in a passive view. Rewind and other unsupported projections still use a full journal scan, and the indexed pager still groups retained projection rows. These paths leave the general closed-history scenario unverified. Keep this change active.

## Sparse domain recovery and frontier advancement

The next increment separates Shell diagnostic observations from the resident Timeline. Their original domain and physical coordinates remain indexed. Cold load folds the retained records, restores the next domain sequence and initial usage timestamp, and validates the unpublished tail. A durable runtime commit advances the manifest under the append lock. Other durable writes advance it after 1 MiB of new journal bytes. The current format also writes and syncs new domain references in a suffix file before replacing the manifest. History range reads seek into both the base and the published suffix.

The Shell process test covers both sides of initial publication and both sides of advancing an existing generation. It checks the same generation after the crash, exact domain range reads across the base and suffix, the runtime phase, retry without duplication, and a torn final line. The focused Shell set passed nine tests. ChatState passed 653 tests with three ignored.

The real terminal fixture holds one model-visible system item and a single closed user/assistant turn constant. It varies only 16 KiB `shell/artifact` diagnostic facts. Three alternating pairs used the prior display-only indexed binary as A and the sparse-domain binary as B. Each session was opened twice in its own home. Table values are medians in milliseconds from the second process launch, with no inference and no exit retries.

| Closed diagnostics | Journal bytes | First screen A → B | Input A → B | Ctrl+D A → B |
| ---: | ---: | ---: | ---: | ---: |
| 64 | 1,068,291 | 170 → 173 | 234 → 208 | 118 → 217 |
| 8,192 | 136,340,689 | 8,401 → 176 | 8,544 → 255 | 121 → 218 |
| 24,576 | 409,048,772 | 24,852 → 186 | 25,103 → 237 | 227 → 218 |

First-open B input medians were 270, 8,867 and 26,020 ms. Before coordination recovery switched to resident facts, the intermediate binary rendered the 131 MiB session at about 175 ms but still delayed input to 7–9 s. The completed paired run uses the corrected startup path.

The final suffix-index binary was also sampled in a separate B-only block against the same fixture, three samples per size. Its repeat-open medians for first screen were 168, 178 and 178 ms, for input 254, 258 and 293 ms, and for Ctrl+D 219, 218 and 318 ms. Exit sample ranges were 218–420, 218–319 and 217–319 ms. The largest first-open median was 26,035 ms to input, including one 67,300 ms sample during a build. The separate block does not establish a strict exit-latency bound.

A separate counterexample on the suffix-index binary keeps the same model Surface and grows closed `Tool::Started` and `Tool::Completed` facts inside a completed Turn. On repeat launch it took 242 ms to echo input at 1,091,579 bytes and 26,136 ms at 418,242,556 bytes. Ctrl+D took 118 and 225 ms in these single samples. Closed tool identity, lifecycle validation, and historical response and compaction dependencies are still rebuilt from retained facts. The full executable snapshot, bounded retained state, and general closed-domain acceptance remain open.

## Write-time publication and surrounding behavior

The no-index 409,048,772-byte diagnostic fixture took 25,897 ms to accept input. The Timeline read and decode span was 25,027 ms and the domain fold span was 4.79 ms. A second cold process accepted input in 238 ms. This separates the initial rebuild from repeat-open time.

The subprocess recovery test now creates an indexed session through the production adapter, writes the fixed Surface and durable runtime request, and crosses the publication threshold with an ordinary buffered diagnostic append. It exits on each side of manifest replacement without opening the session for resume or requesting graceful shutdown. The next process reuses the original generation, restores the suspending runtime and original event sequence, reads the exact archived diagnostic by domain position, and retries that final fact after a torn tail without duplication. A separate regression rejects a same-length rewrite of indexed canonical bytes on cold writer admission.

The focused Shell run passed 188 tests with 5 ignored, with `RUST_MIN_STACK=33554432` for the test runner. It covered the durable ledger, publication, transcript and Trajectory, suspension and continuation, workflow, coordination, and planning. The Pager selection passed 329 tests with 2 ignored after fixing stale completion of a child's transcript snapshot when the root session changed. ChatState passed 653 tests with 3 ignored. These do not verify the complete executable checkpoint or general history paging.

A read-only count of a fixed byte cut in the reported session found about 159 MiB in the canonical journal. The largest categories included presentation (49 MiB), planning (37 MiB), sampling evidence observations (36 MiB), and control (20 MiB). Shell diagnostics accounted for about 1 MiB. The separate sampling artifact directory contained 6,857 files and occupied about 340 MiB on disk when inspected. The storage adapter verifies referenced sampling body chunks during load, so the diagnostic-only size curve does not approximate the reported session's load.

### First resume after ordinary writes

Ran `fixed_surface_resume_size_curve_acceptance` with `GROW_COMPARE_NORMAL_WRITES=1` and `GROW_COMPARE_CLOSED_HISTORY_KIND=domain-observation`. Both fixtures were written through the storage adapter. The baseline arm removed only the derived manifest before launch, reproducing its original lack of publication. The candidate's index was published by ordinary appends. Neither fixture was opened for recovery or gracefully suspended during setup. Both binaries used the same macOS dev profile and `--no-default-features --features sandbox-enforce`. Three alternating pairs per size, 36 PTY launches, zero inference requests and zero exit retries. Values are first-open medians in milliseconds.

| Closed diagnostic journal | First screen A → B | Typed input A → B | Ctrl+D A → B |
| ---: | ---: | ---: | ---: |
| 1,070,442 B | 208 → 209 | 268 → 220 | 217 → 115 |
| 136,545,072–136,545,073 B | 8,671 → 327 | 8,683 → 339 | 322 → 217 |
| 409,677,334 B | 25,355 → 462 | 25,366 → 587 | 218 → 217 |

At the largest size, first candidate input ranged from 466 to 676 ms. Same-home repeat input medians were 235 ms for A and 281 ms for B. Candidate Ctrl+D ranged from 217 to 220 ms at that size. The first-open candidate time still differs across sizes and samples. The test controls the working Surface and history class, with no OS page-cache or host load control. It does not establish the full size-independence criterion or measure general older-page costs. Raw timing and PTY captures are in `/tmp/grow-neighbor-review/pty-equivalent/`.

### Release impact review

The local Shell library suite passed 4,782 tests with 13 ignored. The Pager library suite passed 7,646 tests with 15 ignored before the final transcript fix. After that fix, the Pager transcript, task completion, session load, and reconnect selection passed 221 tests with 1 ignored. It covers selecting an assistant message beyond the resident page, writing the exact result, and discarding a result after rebinding.

## Tool lifecycle checkpoint increment

A published Tool cut restores the open start references and resolves closed start/terminal identities from the same canonical journal. The normal write path also publishes standalone notice ordinals and exact notice IDs. Published domain range reads still recover the original Tool records by sequence.

The subprocess checkpoint regression stops on both sides of manifest replacement with one open tool and one closed tool. Recovery checks the original open call, next event identity, rejected duplicate start, valid fresh start and exact retry after a torn final line. An earlier test run exposed exclusive index handles being reopened while repairing a torn tail or loading twice with the same writer. The writer now shares the index handle across those paths and retains its lease until the blocking append drops its prefix reference.

The crash test also corrupts the canonical record behind a published closed Tool identity. A duplicate start must fail with an indexed read error. The test suite passed 415 Shell storage tests (2 ignored), 11 history projection tests, 17 message-delivery tests, and 653 ChatState tests (3 ignored). Older indexed pages now locate the already-projected group boundary by ordinal without walking the preceding rows on each request.

Early PTY runs reproduced 26-second input readiness at 491,904,680 bytes on `cc9ee17a1`. The first Tool-index candidate reduced repeat input readiness to 263 ms, while its first prompt still took 25,191 ms after Enter. `prepare_user_skill_loads` had been reading every Timeline record to locate a submitted input. It now queries the canonical input fold by identity. Stop and pre-tool Hook restoration also use retained evidence, and the background search refresh cancels its read when its owner exits. One subsequent 491,904,680-byte pair measured repeat input at 25,772 → 254 ms, the first operation at 385 → 253 ms, older page at 283 → 313 ms and Ctrl+D at 255 → 241 ms. Two of three 140,590,758-byte candidate repetitions in that sweep still took about 7 seconds for the first operation. The original probe submitted identical text in both processes and sent Ctrl+D immediately after the mock response marker. The following sweep uses different explicit prompts and measures Ctrl+D after a rendered idle boundary. It keeps all first and repeat observations, including slow ones.

Published sampling audit observations and provider bodies are also outside the indexed execution fold. A separate storage regression removes a published historical body, resumes via the index with the same domain frontier, reads the original reference by sequence, and verifies that a full load rejects the missing body. The bounded unpublished tail and execution-required prompt blobs retain their checks.

This increment still folds retained non-Tool execution facts and eagerly projects dependent response and Tool display rows. General executable working-state serialization, independent migration of old sessions, and readiness independent of the latest page remain open. The change stays active.

### Closed Tool lifecycle terminal A/B

Three alternating pairs per size, 36 macOS PTY launches, with A pinned to `cc9ee17a1` and B built after exact Tool indexing, input lookup, background search cancellation, and page grouping. Both used the same dev build configuration (`--no-default-features --features sandbox-enforce`), isolated homes, 50×120 PTYs, and a loopback mock provider. Each arm wrote its journal through its own storage adapter and retained the published manifest. Every sample had the same two-item Surface digest, 144 fixed short notices, and no open Tool. Only the closed Tool lifecycle pairs increased. Each launch echoed text without inference, then rendered the response to one explicit request. Ctrl+D was timed from the two keys sent after the prompt became idle to observed exit and terminal drain. All 36 launches exited successfully, and every recorded trial issued one foreground request per launch. An additional candidate build includes sampling audit eviction and the historical checkpoint ownership check. Those two changes were verified with targeted storage regressions and are outside the paired CLI measurement.

Medians in milliseconds. Each cell shows A → B. Older page was measured after repeat open and selected fixed notices. The fixture has no growing ACP Tool presentation groups.

| Journal bytes | Open | First screen | Input echo | First operation | Ctrl+D | Older page |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1,184,186 | First | 212 → 217 | 261 → 233 | 292 → 300 | 168 → 135 | — |
| 1,184,186 | Repeat | 182 → 185 | 259 → 221 | 252 → 232 | 184 → 132 | 301 → 302 |
| 140,590,758–759 | First | 7,557 → 222 | 7,681 → 237 | 321 → 291 | 7,467 → 131 | — |
| 140,590,758–759 | Repeat | 7,525 → 190 | 7,690 → 237 | 288 → 246 | 7,465 → 134 | 298 → 304 |
| 491,904,680 | First | 25,109 → 425 | 25,678 → 492 | 414 → 314 | 12,239 → 137 | — |
| 491,904,680 | Repeat | 25,020 → 193 | 25,485 → 260 | 415 → 273 | 12,249 → 132 | 294 → 266 |

At the largest size B's first-open input samples were 489–603 ms and repeat samples were 251–289 ms. B had one 5,405 ms first-open input sample at the smallest point. The 492 MB first-open median is higher than both smaller medians. These data remove the seconds-per-hundreds-of-MB repeat-open behavior for this fixture and do not establish a size-independent first-open bound. The harness recorded wall-clock latency and exact fixture bytes. It did not record bytes read, peak RSS, or a controlled filesystem cache state. The whole-contract acceptance and archive remain open. Raw measurements and first-screen casts are under `/private/tmp/grow-tool-pty-acceptance-v11/`.

### Attribution rerun of the 233–492 ms first-open gap

The original candidate casts show the first terminal byte 16–18 ms after PTY spawn returned at the small and medium points, excluding the 5.4 s small outlier. At 491,904,680 bytes all three first opens emitted the first byte after 118–126 ms. Their first synchronized frame began at 225–261 ms, versus 52–62 ms at the smaller points. These intervals precede display of the recent page. The XTVERSION query preceded the first frame by roughly 8–10 ms on the large runs.

In a separate instrumented run of the same v11 executable, the first-open input medians were 273, 546 and 530 ms. Repeat-open medians were 282, 262 and 262 ms. The first large launch took 520 ms to echo input, including 227 ms for Shell's load handler, 60 ms in Timeline read/decode, 16 ms for the replay path and 0.9 ms for history projection. Across the three large first opens the handler took 227–254 ms and read/decode took 60–77 ms. The medium first opens took 204–218 ms and 41–53 ms respectively. Running the candidate's `--version` immediately before the two later large launches did not remove the extra time. The evidence places most of the original increase before the recent-page frame and rules out cumulative response projection as its main component. Startup/cache and scheduling effects still require narrower attribution. Raw stage logs, casts and the `--version` wrapper are under `/private/tmp/grow-tool-stage-profile-v11/`.

### Deferred latest page and migration

The Pager reducer regression verifies that an ordinary live prompt reaches the send effect while the newest page is still pending. It then injects a page failure, retries and checks insertion ahead of live output. The latest-page response is observational and scoped to the loaded binding. The fullscreen cold fast path currently defers page delivery for an indexed root with no active Turn, descendant lifecycle or unresolved background-task presentation. Resident reconnect and the other renderer paths still use their existing delivery path.

The background migration builds the index before claiming the writer lease, then rechecks the original journal and directory at publication. The storage regressions cover a live writer holding its lease, canonical byte preservation, cross-page Tool terminal reconstruction from a committed `InProgress` field update, canonical response references and the pre/post-publication process-exit cases. The migrated publication still contains the retained history fold. Arbitrary closed Request, Hook, Input, usage and child facts remain on that path, and response ordering still derives from the retained fold.

### Instrumented terminal A/B at 497.5 MB

One A/B pair per size on macOS PTYs, with one first open and one repeat process per arm. A is the same `cc9ee17a1` baseline. B is the final candidate built with `--no-default-features --features sandbox-enforce`. The two-item Surface hash was identical across all six arms, and both writers used the normal commit path. Only closed Tool lifecycle pairs grew, from 64 to 8,192 and 29,000. The harness sent the typing probe as soon as the load-ready frame was seen, independent of the recent-page marker. Ctrl+D was measured after the first foreground operation returned to idle. All 12 processes exited successfully.

Single-sample latencies in milliseconds, A → B. “Recent page” is the first terminal frame with the history marker. The immediately preceding loading or ready frame is not counted as the recent page.

| Journal bytes | Open | Recent page | Input echo | First operation | Ctrl+D | Older page |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1,184,186 | First | 569 → 6,423 | 730 → 6,314 | 414 → 495 | 218 → 132 | — |
| 1,184,186 | Repeat | 203 → 337 | 324 → 244 | 308 → 256 | 143 → 131 | 280 → 262 |
| 140,590,758 | First | 7,804 → 607 | 8,035 → 499 | 406 → 329 | 7,241 → 134 | — |
| 140,590,758 | Repeat | 7,470 → 369 | 7,796 → 249 | 612 → 294 | 7,167 → 133 | 277 → 281 |
| 497,531,191 | First | 26,369 → 700 | 26,954 → 579 | 461 → 368 | 12,250 → 135 | — |
| 497,531,191 | Repeat | 25,887 → 433 | 26,470 → 299 | 410 → 269 | 12,250 → 136 | 276 → 274 |

B hydrated three retained records totaling 1,044 bytes at all three sizes. On first open the unpublished tails were 133,044, 304,692 and 664,927 bytes. Their hydration took 11, 34 and 62 ms, respectively. Index open took 10, 15 and 22 ms. These measurements show the additional bounded tail work that the earlier combined read/decode timer could not distinguish. At 497.5 MB, Pager's measured pre-event-loop setup took 167 ms on first launch and 7 ms on repeat. Embedded ACP connection accounted for 108 ms and 3 ms of those times. The small first-open outlier already spent 6.05 seconds before emitting the first terminal byte. Its subsequent instrumented Shell load took 154 ms.

At the largest point input arrived 121 ms before the recent page on first open and 134 ms before it on repeat. B's repeat input and recent-page times still rose across this sample, even though the retained record count was constant. Tail length and startup conditions were uncontrolled. The measurement supports bounded Tool lifecycle replay work and the separation of readiness from recent-page delivery for this path. It does not establish the full size-independent terminal acceptance or explain the remaining first-open startup variance. Logs, casts and phase values are under `/private/tmp/grow-working-state-final-stages/`.

Verification also passed 201 JSONL storage tests with one ignored, 11 history projection tests, 68 Pager load reducer tests and the observational older-page reducer test. Strict OpenSpec validation passed for all 20 active changes/specs and 957 archived changes. The general checkpoint and transport-bounded response continuation remain open, so this change is still active.

### Fresh executable control on macOS

The v10 candidate's first small PTY run emitted its first byte at 7.25 s. Pager's measured startup was only 85 ms and the Shell load handler took 225 ms. To isolate the time before Pager, a control ran `--version` twice on each of three APFS clones of the same 549 MiB executable. The first launches took 6,730, 7,283 and 7,337 ms. Repeat launches took 14–16 ms. `--version` exits before starting the runtime or opening a session. This reproduces the multi-second outlier independently of history and narrows it to first launch of a fresh executable. The control does not distinguish loader and macOS verification costs. It also does not explain the smaller 233–492 ms difference on subsequent session fixtures. Raw control values are in `/private/tmp/grow-working-state-v10-stages/fresh-executable-control.json`.

### Final build terminal A/B with first frame measured separately

The same three sizes ran after the cross-page `InProgress` fix. This was one instrumented pair per size with an isolated fixture and a first and repeat process per arm. A and B were built from the same dev profile as above. B's SHA-256 was `83482e46ab65ccdd6306c36d761b20544574751599e0e0f9efa6f96e8a55aed6`. All 12 processes exited successfully. The history sizes and two-item Surface digest matched across all arms. Latencies are milliseconds, A → B.

| Journal bytes | Open | Terminal first frame | Recent history on screen | Input echo | First operation | Ctrl+D | Older page |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,184,186 | First | 546 → 7,343 | 932 → 7,690 | 1,352 → 7,608 | 628 → 580 | 229 → 236 | — |
| 1,184,186 | Repeat | 83 → 59 | 264 → 375 | 451 → 279 | 422 → 318 | 233 → 247 | 273 → 269 |
| 140,590,758 | First | 90 → 92 | 7,313 → 392 | 7,474 → 284 | 891 → 649 | 6,365 → 255 | — |
| 140,590,758 | Repeat | 39 → 42 | 7,188 → 406 | 7,398 → 284 | 375 → 318 | 7,170 → 158 | 270 → 273 |
| 497,531,191 | First | 455 → 280 | 26,172 → 716 | 26,845 → 581 | 655 → 517 | 12,237 → 258 | — |
| 497,531,191 | Repeat | 51 → 63 | 25,253 → 465 | 25,800 → 308 | 697 → 316 | 12,337 → 135 | 270 → 277 |

At 140.6 and 497.5 MB, B's first-open input differed by 297 ms. Embedded ACP connection grew from 41 to 107 ms. The Shell load handler grew from 179 to 259 ms, including the indexed replay of a 305 vs 665 kB suffix (21 vs 56 ms) and index open (11 vs 27 ms). Recent history projection took 0.14 and 1.34 ms. Three retained records totaling 1,044 bytes were identical at all sizes. The measured connection and load intervals explain part of the first-open gap. Time outside those intervals, including time before Pager starts and client-side scheduling, still varied. These samples shared the host with the earlier uninstrumented sweep and some verification work. They do not isolate cache state or prove a size-independent terminal bound. Raw timings and casts are under `/private/tmp/grow-working-state-v10-stages/`.

### Request and accounting checkpoint increment

The published Request index distinguishes evidence before and after a domain position. The storage regression resumes a published open Request with its terminal in the durable suffix, restores a response whose completed Request is absent from resident events, compares Surface identities against dense replay, and rejects reuse of both Request identities. It corrupts the published terminal and checks that exact validation returns a history error. A separate accounting regression compares the checkpoint plus a resume marker and exact settlement retry in the suffix to a dense fold and verifies a conflicting retry fails. Growing closed Request and usage facts from 1 to 128 leaves exactly three resident domain records while matching the dense Surface identities and usage ledger. Process-exit tests cover both sides of manifest replacement with an open Tool, a closed Request and accounted usage. The focused storage suite passed 12 tests. ChatState passed 653 tests with 3 ignored.

Tool presentation order now indexes terminal Tool rows. Their original start and field updates are verified on demand, and hidden intermediate update identities still participate in exact deduplication. A large-response regression reconstructs escaped and multibyte text across byte continuations and verifies the encoded size of each returned update array. This still decodes the original canonical record before slicing its text. Response ordering still runs in the retained projection, and an oversized non-response row cannot yet be split.

Hook, Input and child lifecycle facts are still replayed in the cold path. A completed Hook can still be the authority for a pending admission or suspended Turn. A consumed Input may own a live Surface item and its historical payload remains an attachment retention root. A terminated child may still owe delivery, usage or planning settlement. The current accounting checkpoint also includes old per-incarnation segments and per-agent totals. The tests below isolate closed Tool facts and do not close these requirements.

### Fixed unpublished suffix terminal A/B

A is the pinned `cc9ee17a1` product binary. B is the v14 working-index candidate, SHA-256 `3144eaee69ca48f5218b00a36c610719cb530ed1615e4a1c64208b7ca8d4f08e`. Both use the macOS dev profile with `--no-default-features --features sandbox-enforce`. The test exercised fullscreen Grow through real 50×120 PTYs with separate homes and a local mock provider. It warmed both executable paths with `--version`. It rotated the size order and interleaved A and B.

Each arm used its own version's normal writer and published its own index before opening the terminal. Both writers appended the same inert publication-forcing record, then durably appended a Step terminal and a Turn terminal beyond the manifest. At a given size the two tail records must have identical bytes across all arms. Across sizes their content must match after removing physical and domain sequence digits and the dependent checksum. The first-open suffix measured 665 bytes for 64 Tool pairs and 673 bytes for both larger sizes. The model Surface hash must remain identical. A same-home repeat starts after the test's new mock operation and graceful suspension, which add a bounded set of facts and change the suffix.

The first synchronized frame can be a loading screen. The recent-history column waits for the fixture marker in an actual frame. The harness types before the recent-page marker is required. The first-operation duration starts when a fresh prompt is explicitly submitted after both milestones. On repeat open, this follows the older-page probe. Ctrl+D duration starts when both EOF keys are sent after the foreground operation settles and ends after successful process exit and the harness's PTY drain.

All 36 terminal processes passed. The following values are medians of three pairs per size, in milliseconds, A → B.

| Journal bytes | Open | Terminal first frame | Recent history | Ready | Input echo | First operation | Ctrl+D | Older page |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2,233,069–2,233,070 | First | 239 → 98 | 533 → 366 | 618 → 268 | 629 → 279 | 555 → 426 | 213 → 239 | — |
| 2,233,069–2,233,070 | Repeat | 38 → 47 | 198 → 344 | 283 → 251 | 294 → 262 | 413 → 282 | 220 → 136 | 277 → 273 |
| 141,639,645–141,639,646 | First | 257 → 70 | 7,748 → 318 | 7,858 → 216 | 7,869 → 226 | 7,533 → 334 | 142 → 133 | — |
| 141,639,645–141,639,646 | Repeat | 49 → 52 | 7,504 → 317 | 7,625 → 212 | 7,636 → 223 | 7,513 → 258 | 141 → 133 | 275 → 275 |
| 498,580,078–498,580,079 | First | 141 → 311 | 26,057 → 623 | 26,553 → 528 | 26,565 → 539 | 25,769 → 399 | 330 → 132 | — |
| 498,580,078–498,580,079 | Repeat | 48 → 47 | 25,717 → 348 | 26,283 → 253 | 26,294 → 263 | 25,838 → 270 | 330 → 134 | 277 → 269 |

B folded 11 retained records at each size, occupying 4,085–4,119 bytes before replaying the suffix. At 142 and 499 MB the first-open tails were both 673 bytes. First-open hydration medians were 2.4, 1.3 and 15.5 ms, and index-open medians were 7.8, 11.9 and 22.3 ms. Index open has a remaining size signal. The pinned redb version visits allocated pages during debug open even with a valid saved allocator. This is a plausible source of the index-open signal, distinct from canonical replay. The current logs time the whole index open and do not isolate that visit.

Candidate first-open input ranged from 222–437, 223–462 and 466–904 ms. Pair 0 at 142 and 499 MB had the same tail length, near-identical startup spans (129 and 128 ms) and hydration (14.6 and 14.5 ms). Input arrived at 462 and 466 ms. At 499 MB the 904 ms sample emitted its first terminal byte at 463 ms from the cast's start, compared with 121 ms for the 466 ms sample. It spent 226 ms before Pager's event loop and 246 ms in Shell load, compared with 128 and 186 ms in the faster sample. Embedded ACP connection explains 15 ms of the 98 ms pre-loop difference. Index open and hydration explain 11 ms of the 60 ms Shell difference. The terminal and instrumentation clocks have different origins, so these spans cannot be summed with cast timestamps. The first frame in the 904 ms sample already took 619 ms, before the recent page. The fresh-executable control above reproduces multi-second startup outliers independently of the journal.

Repeat open removes the 26-second regression for the closed-Tool fixture. The fixture grows canonical Tool execution facts and keeps its small set of display rows fixed. This isolates execution replay and does not measure growing response or Tool display groups. First open still shows a higher median at the largest point. The full checkpoint and general history pagination requirements remain open. Raw samples, casts, and internal phase logs are under `/private/tmp/grow-resume-fixed-tail-ptys/`.

### Ctrl+D without a preceding operation

A separate PTY control uses fresh fixtures with the same Surface and suffix, one pair at the smallest and largest sizes. It waits for the recent history marker and the idle ready prompt, submits no prompt, and sends Ctrl+D twice. This measures exit before a foreground operation can warm a history path or change the fixture.

| Journal bytes | Ctrl+D without a preceding operation A → B, ms |
| ---: | ---: |
| 2,233,069 | 421 → 131 |
| 498,580,078 | 236 → 336 |

All four processes exited successfully with zero retries and zero inference requests. This one-pair control shows variation at direct exit and cannot establish a size-independent bound. The larger trial overlapped activity from a separate repository's test suite. Raw measurements and stage logs are under `/private/tmp/grow-resume-fixed-tail-cold-exit/`.

### Published Hook, Input, child, usage and display slices

The Shell JSONL selection passed 210 tests with two ignored. The new Hook/Input case writes 128 dismissed inputs with completed prompt gates, holds another completed prompt gate across publication and admits its original input in the unpublished suffix. Recovery retains four domain facts, restores the exact FIFO admission and rejects an archived input identity reuse. The child index regression stores 40,000 lifecycle facts for 20,000 settled children and leaves one unresolved spawn in the checkpoint. The usage tests verify historical segment lookup, per-child attribution, exact settlement and a late settlement applied to its original segment.

Response tests read and verify the exact requested canonical text fragments, including escaped text and multibyte characters. They reject a changed journal byte within a selected fragment. The oversized Tool test walks the same byte continuation cursor as interactive paging and reconstructs the original merged payload. A normal Tool with a field supplied by an earlier in-progress update and publication with no oversized Tool both have regression coverage. The first release sweep exposed a missing table for an ordinary page, corrected by creating the fragment tables in the publication transaction. A second build exhibited a missing page in the real terminal. The instrumented rebuild now records the Shell page read and Pager application separately.

The cold loader still iterates the retained record table. Closed Input submissions, admission decisions and terminal identities have exact lookups, but consumed input Surface operations, older message facts, and Turn, Step, Runtime, Planning and Control evidence still contribute to the retained table. The known fixed-Surface counterexample is a growing number of closed turns or compacted-away message operations. The Tool-only size curve holds those records constant and cannot establish the general working-state requirement. Eviction requires exact Turn and Step identity lookups and a validated Runtime and Surface projection that preserves suspended continuation and rewind provenance. Existing full-history verification remains applicable to those paths.

The 515,977,893-byte publication regression passed on the ordinary writer, with two real child-process stops. Before manifest replacement the existing generation kept its old cut, and the loader applied the complete later fact at its original sequence. An exact durable retry published that fact without duplication. After replacement the next process selected the same generation at the new frontier. The eight retained records were identical in count at both cuts. Measured recovery was 173.2 ms before and 24.9 ms after replacement, with a 2 MiB unpublished fact in the former case. This test checks process loss at the publication points. It does not simulate a power failure.

The full ChatState library run passed 656 tests with three ignored, including bounded segment count, matching dense accounting, and settlement in the original segment. The Pager regressions confirmed a delayed/failed initial page can be retried after live input and that historical Tool fragments project as passive Tool cards. During review, the automatic status broadcast was found to ask for the full usage ledger. It now reads the aggregate projection. Explicit usage requests still materialize historical segments. Input artifact cleanup now checks candidate hashes in bounded batches, and its historical image-root inspection stops on session shutdown.

### Release PTY comparison through 515.7 MB

A was release `cc9ee17a1` (SHA-256 `5ac611d9d5a83a90e3fea7f5dcfae9636e064cebab4d493d56c0185c29fd074c`). B was the instrumented version 15 candidate (SHA-256 `c1f53d74e3b8459b46cea24b7af111df702b92b271796e999f77b475266730be`). Both were optimized macOS builds with `--no-default-features --features sandbox-enforce`. Three pairs per size, 36 successful process launches. The same working Surface and two durable Step/Turn tail facts were verified across arms. The unpublished suffix was 665 or 673 bytes. Only the number of already closed Tool lifecycle pairs varied. The host also ran compilation and correctness tests, so timings include uncontrolled scheduling and cache effects.

Medians in milliseconds, A → B. First frame is the first synchronized terminal frame, which can be the loading screen. Recent history requires the actual history marker.

| Journal | Open | First frame | Recent history | Input echo | First operation | Ctrl+D after operation |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | First | 135 → 71 | 282 → 178 | 322 → 178 | 325 → 307 | 223 → 245 |
| 2.23 MB | Repeat | 45 → 45 | 144 → 123 | 157 → 123 | 181 → 244 | 122 → 247 |
| 141.64 MB | First | 414 → 177 | 1,285 → 369 | 1,607 → 366 | 997 → 432 | 233 → 252 |
| 141.64 MB | Repeat | 40 → 62 | 690 → 224 | 945 → 224 | 807 → 354 | 248 → 259 |
| 515.73 MB | First | 174 → 208 | 1,729 → 478 | 2,105 → 478 | 1,674 → 569 | 295 → 286 |
| 515.73 MB | Repeat | 43 → 51 | 1,490 → 285 | 1,778 → 273 | 1,558 → 380 | 276 → 283 |

B first-open median index open was 9.0, 8.2 and 7.5 ms. Median hydrate was 0.4, 7.9 and 13.3 ms, with 11 retained records and 4,085–4,119 retained bytes. The latest-page read medians were 7.1, 14.6 and 9.1 ms. Pre-event-loop startup took 38.0, 90.9 and 122.8 ms, including embedded connection medians of 34.3, 74.8 and 95.1 ms. The terminal query was 0.2–0.7 ms median. Shell load took 92.6, 168.9 and 215.1 ms. The release run eliminates the prior size trend in index open for these points. It still shows a wall-clock difference between the smallest and largest sessions. Setup, embedded spawn/initialize and Shell scheduling contribute to the first-open variation. The exact cause of each scheduling interval is not isolated by this run. Logs and casts are under `/private/tmp/grow-resume-v15-c-release-ptys/`.

The version 16 candidate's release executable was cloned three times and launched with `--version`, which exits before opening a session. The first launches took 2,855, 3,114 and 6,054 ms. Repeating the same paths took 7–8 ms. The pinned release baseline yielded 3,915–5,605 ms on first launch and 7–9 ms on repeat. This control independently reproduces a macOS executable first-launch effect. It cannot determine why all of the smaller historical 904 ms sample's startup and Shell intervals were longer. Raw control timings are in `/private/tmp/grow-resume-v16-executable-control.jsonl`.

The direct idle Ctrl+D control completed on the same version 15 candidate with no foreground request, no exit retries and exit status zero. At 2.23 MB A → B was 218 → 136 ms; at 515.73 MB it was 235 → 758 ms. There was one pair at each size and overlapping host activity during the larger point. This still leaves the direct-idle latency acceptance open. Raw samples are in `/private/tmp/grow-resume-v15-c-release-idle-exit/`.

### Release PTY repeat with manifest version 16

A was the same `cc9ee17a1` release. B was built at manifest version 16 with SHA-256 `8ae4d81262aed850e7f0d87b180de30809c547919868d6022bbca41be0197e0f`, before the later child operation query changes. The fixture writer and B both used manifest version 16. All 36 launches completed. Each point used three interleaved pairs and the same two-item model Surface, with two complete unpublished facts. The logical suffix was identical and the exact bytes matched across both arms at each size. B decoded 11 retained records, 4,085–4,119 retained bytes and 665–673 tail bytes at all first-open sizes.

Medians in milliseconds, A → B.

| Journal | Open | First frame | Recent history | Input echo | First operation | Ctrl+D after operation |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | First | 78 → 68 | 186 → 174 | 198 → 174 | 217 → 222 | 122 → 142 |
| 2.23 MB | Repeat | 40 → 43 | 110 → 104 | 123 → 104 | 179 → 145 | 122 → 243 |
| 141.64 MB | First | 84 → 70 | 484 → 181 | 600 → 181 | 586 → 205 | 131 → 234 |
| 141.64 MB | Repeat | 38 → 38 | 449 → 143 | 570 → 143 | 557 → 182 | 239 → 137 |
| 515.73 MB | First | 101 → 84 | 1,535 → 158 | 1,809 → 161 | 1,593 → 253 | 275 → 139 |
| 515.73 MB | Repeat | 38 → 37 | 1,389 → 127 | 1,741 → 127 | 1,647 → 190 | 288 → 137 |

Median B hydration was 0.35, 0.41 and 0.38 ms. Median recent-page reads were 6.62, 6.58 and 6.96 ms. Median B older-page roundtrips after repeat open were 299, 290 and 299 ms. The separate fresh idle Ctrl+D comparison took 220 → 133 ms at 2.23 MB and 238 → 240 ms at 515.73 MB. All four idle processes exited with status zero, zero inference requests and zero retries. This pair does not isolate the remaining idle-exit variation. The closed-Tool fixture establishes fixed read work on that domain path. It holds closed Turn/Step and compacted-away message facts constant and does not verify general scaling. Raw PTY casts and stage logs are under `/private/tmp/grow-resume-v16-final-ptys/` and `/private/tmp/grow-resume-v16-final-idle-exit/`.

In version 16, the ignored `closed_turns_do_not_expand_the_executable_checkpoint` test isolated a remaining dependency. It writes a one-item Surface and keeps the session idle at the published cut with an empty tail. Raising the number of closed Turn/Step pairs from one to 128 raises both published retained records and the resident fold from 5 to 513. The test failed on the growth assertion at that version. `settled_child_is_available_to_exact_queries_after_cold_load` passed and verifies that an omitted child remains addressable by its original ID. Runtime tree queries also merge any archived lifecycle facts named by the active suspension barrier. Explicit child history reads merge the archived facts on a blocking worker. Historical child presentation remains a separate dependency.

After the child query changes, Shell JSONL passed 210 tests with three ignored, descendant recovery passed eight tests, runtime handoff passed 13 tests with five ignored, and ChatState passed 656 tests with three ignored. The 515,977,893-byte process-crash test passed again on this build. Recovery took 158.7 ms before manifest replacement and 30.2 ms after it. Both cuts retained eight records, and the exact retry preserved the original fact without duplication. Strict validation passed 20 active changes/specifications and archive validation passed 957 entries. The change still has open implementation and acceptance tasks.


### Turn, Runtime and Surface publication

Manifest version 19 publishes the current Surface, original item IDs, revision and pending Control contexts. Runtime phase, wait state and active Turn progress advance with the same transaction. Ordinary message bodies and closed Turn/Step and Runtime facts leave the retained table where they have no retained consumer. Response admission and boundary identities point to the original verified journal records. Explicit rewind reconstructs canonical branch evidence. Image projection and already rewound branches still use dense recovery.

The original closed Turn/Step counterexample now passes as a regular test. At one and 128 closed Turns, the published retained count and resident event count both stay at zero. The restored Surface, prompt frontier and next domain sequence match dense replay, and the test checks rejection of archived Turn identity reuse and exact history at the original sequence. The compaction fixture checks matching Surface item IDs and revision, the exact archived original body and matching rewind output.

A 515,977,893-byte journal written through the ordinary adapter passed real process exits before and after replacing the manifest. Recovery preserved the generation and the exact suffix event across a durable retry. The two published cuts retained zero replay records. The first version 19 run measured 162.7 ms before replacement with a 2 MiB tail and 67.3 ms after replacement. The final Shell test build repeated both process stops at the same size and measured 179.3 and 41.2 ms, with zero retained replay records at both cuts. Shell JSONL tests passed 212 with two ignored, including a Stop continuation already committed by an archived Step. ChatState passed 656 with three ignored. Descendant recovery passed 8 tests, runtime handoff passed 13 with 5 ignored, history projection passed 11, permission Auto mode passed 22, message delivery passed 17, and image asset reconciliation passed 8.

The cold loader still decodes published consumed Input bodies, historical Control/Planning, compaction evidence and retained presentation rows. For example, adding one admitted and consumed Input per older Turn and then compacting it out of the Surface still adds one retained domain row and one user echo per Turn. Adding a closed Control activation or a completed compaction can retain additional boundary pairs. The child index also tracks settlement through explicit receipt consumption or dismissal, while the Timeline can retire a completion receipt through a delivered Tool result. A child settled through that path may still enter the published working set. A ContextRecall Sideband can also require a dense branch reconstruction during validation. The Auto permission classifier still reconstructs canonical authority when requested. Normal Surface materialization no longer requests permission history, and the background image sweep reads canonical committed paths when an asset directory exists. The closed-Tool terminal fixture cannot establish the size bound for these paths. The OpenSpec change remains active while those acceptance cases are open.

### Manifest version 19 release PTY comparison

A was release `cc9ee17a1` (SHA-256 `5ac611d9d5a83a90e3fea7f5dcfae9636e064cebab4d493d56c0185c29fd074c`). B was the current optimized candidate (SHA-256 `b97ac6344c6d500a4b5cefb34870315a1ac87a304a581efede12d73076b2f314`). Both used `--no-default-features --features sandbox-enforce`. The macOS PTY harness ran three interleaved A/B pairs at each of three sizes, with one first open and one repeat per arm. Both fixtures used their production writers and verified the same working Surface and two durable unpublished terminal facts. Only the count of closed Tool lifecycle pairs grew. The two logical tail facts were fixed, with identical bytes across arms at each size. The loopback provider answered one explicit foreground operation per launch.

Medians in milliseconds, A → B. Recent history requires the newest history marker in a complete terminal frame. The first frame may be the loading screen.

| Journal | Open | First frame | Recent history | Input echo | First operation | Ctrl+D after operation | Older page |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | First | 68 → 77 | 166 → 180 | 179 → 180 | 265 → 199 | 225 → 140 | — |
| 2.23 MB | Repeat | 43 → 43 | 107 → 119 | 130 → 132 | 165 → 389 | 123 → 135 | 281 → 283 |
| 141.64 MB | First | 177 → 80 | 605 → 184 | 744 → 194 | 585 → 400 | 132 → 147 | — |
| 141.64 MB | Repeat | 42 → 44 | 452 → 113 | 549 → 124 | 515 → 166 | 130 → 140 | 288 → 278 |
| 515.73 MB | First | 117 → 97 | 1,460 → 192 | 1,809 → 205 | 1,588 → 214 | 278 → 140 | — |
| 515.73 MB | Repeat | 40 → 43 | 1,385 → 114 | 1,724 → 121 | 1,564 → 163 | 281 → 137 | 282 → 281 |

B first-open median Timeline read/decode was 28.70, 23.98 and 12.17 ms at the three sizes. Hydration read 2 retained rows totaling 706–711 bytes and a tail of 665–673 bytes. Its median was 0.34, 0.39 and 0.42 ms. First-open candidate input ranged from 170–375, 184–444 and 172–267 ms. The largest session's first operation ranged from 205–229 ms and its post-operation exit from 134–143 ms. The two smaller points include transient host contention. The retained bytes and timings show no linear cold-storage scan for closed Tool lifecycle. They do not establish the same result for retained Input, Control, compaction, child-delivery or other presentation history. Raw timings and casts are under `/private/tmp/grow-resume-v19d-release-ptys/`.

The direct idle Ctrl+D sweep used the same release binaries and repeated three pairs at every size. Each of the 18 processes exited with status zero, without a retry or an inference request.

| Journal | A median (range), ms | B median (range), ms |
| ---: | ---: | ---: |
| 2.23 MB | 219 (117–220) | 136 (134–138) |
| 141.64 MB | 221 (119–224) | 239 (130–443) |
| 515.73 MB | 228 (225–229) | 139 (131–341) |

The original test process completed all trials, then failed its assertion that all six complete journals at a point had byte-identical length. Fixture metadata changes total length by one byte between some pairs. The paired journal sizes and exact tail hashes matched between arms at each point. The harness now permits that one-byte variation across trials. The revised harness compiled and the full captured dataset passed the revised comparison offline. Raw timings, casts and the revalidated summary are under `/private/tmp/grow-resume-v19d-release-idle-exit/`. The exit spread at the medium and large points remains visible.


### Direct open before background migration

A separate macOS PTY probe opened a synthetic legacy journal of 524,298,752 bytes with no prepublished manifest. It held the Surface fixed and added 31,500 closed diagnostic observations. The same version 19 candidate and loopback route opened the same session twice. On the direct first open, recent history appeared in 4,955 ms and input echoed in 4,963 ms. The Timeline read/decode stage took 1,397 ms, and the full Shell load handler took 4,373 ms including initial publication. On the next process launch, recent history appeared in 215 ms and input echoed in 220 ms. Both runs exited normally without inference or exit retries. This sample measures the legacy direct-open cost separately from the production-written closed-Tool acceptance above. Background publication and its writer-lease race are covered by the storage regression. The legacy journal was synthetic and OS cache state was uncontrolled. Raw timings and terminal output are under `/private/tmp/grow-v19d-legacy-probe/`.

### Direct idle Ctrl+D rerun with the corrected harness

The corrected release harness repeated all nine A/B pairs with the same two Grow executables. All 18 processes exited normally. No process submitted a model request or retried exit. The three candidate medians were 135, 137 and 137 ms.

| Journal | A median (range), ms | B median (range), ms |
| ---: | ---: | ---: |
| 2.23 MB | 222 (117–225) | 135 (131–245) |
| 141.64 MB | 222 (120–223) | 137 (134–144) |
| 515.73 MB | 228 (228–235) | 137 (135–237) |

Raw timings and terminal captures are under `/private/tmp/grow-resume-v19d-release-idle-rerun/`. The fixture varies closed Tool lifecycle only. The broader closed-history task remains open.

## Indexed Input, response candidates and execution projections

The consumed-input regression published the provisional echo on one side of the cut and the consuming fact on the other. Replay validates the ordered echo fingerprint across the cut. The original row appears exactly once before and after advancing. A response regression published a candidate before a Tool terminal and admitted its response afterward. Exact retries and both recovered pages preserved the original candidate order.

The closed-boundary regression now adds a settled Stop Hook for each Turn. One and 128 closed Turn/Step pairs still hydrate the same number of resident events, while exact history returns the original Hook and boundary coordinates. The Planning regression writes 128 revisions, verifies the latest published plan and rejects a stale predecessor. Another regression reinstates only the latest analytics and announcement snapshots and checks that newer observations in the suffix win. The Interaction regression restores one open attempt after 128 completed attempts, rejects reuse of a historical attempt and preserves the Runtime wait after a suspended resolution. A page regression reads 130 historical Turn completions across the recent and older pages from disk in physical order.

Manifest 25 passed 22 working snapshot tests with one manual large crash test excluded from that invocation. The direct launch of the manual test for manifest 21 used a 515,977,893-byte journal and recovered in 155.0 ms before and 30.9 ms after manifest replacement. Neither recovered cut decoded a retained record. This earlier crash sample measures the publication protocol independently of the subsequent projection changes.

The remaining cold-path dependencies include compaction Sideband proof validation, Control receipts and child-delivery presentation. Those paths still need tests where only the corresponding closed history grows. The closed Tool test cannot support that general claim.

The manifest 25 build passed the 515,977,893-byte process-crash test again. Before manifest replacement it recovered in 162.8 ms. After replacement it recovered in 28.9 ms. Both cuts decoded zero retained records, validated exact retry and retained the original domain identities. ChatState passed 656 tests with 3 ignored. Runtime handoff passed 13 tests with 5 ignored after switching live root child waits to the exact operation lookup.

### Manifest 22 terminal control

The macOS PTY comparison used paired release binaries and the normal writers at 2.23, 141.64 and 515.73 MB. The largest first-open recent-history, input and first-operation medians were 1,528 → 318, 1,945 → 318 and 1,560 → 222 ms. Candidate input medians across all sizes were 259, 374 and 318 ms. The three idle Ctrl+D medians were 220 → 239, 226 → 242 and 449 → 341 ms. The idle run overlapped validation and a large fixture build. It cannot decide whether the 102 ms difference is machine load or a release-path dependency. Raw terminal captures and stage logs are in `/private/tmp/grow-resume-v22-final-ptys/` and `/private/tmp/grow-resume-v22-final-idle-exit/`.

### Manifest 29 execution verification

The original 5-to-513 resident replay counterexample now keeps zero published retained rows for both one and 128 closed Turn/Step pairs, including settled Stop Hooks. A second fixture holds the same Surface, active Turn, active Step and open Tool while increasing the number of closed tools inside the active Step from one to 128. Both restore the same three active facts and zero retained records, and reject reuse of the original Tool identity.

The compaction roundtrip compares the source Surface IDs and revision, then exercises explicit rewind against dense replay. An archived Sideband ledger removed after the cut is not opened for execution recovery. The full validation path still reports the missing proof. A child delivery regression applies parent terminal, usage settlement, receipt and ToolResult through the ordinary writer. The cold execution projection omits the settled lifecycle and receipt, while exact queries recover the original child result and consuming Turn. The task inbox regression recovers one pending completion next to 64 settled receipts and checks exact retries, earlier consumers and suppression of a late running notice after terminal settlement.

On 515,977,893 bytes of closed Tool lifecycle history, both subprocess crashes recovered. The process stopped before or after manifest replacement. Recovery preserved zero retained replay records on both cuts and took 651.4 ms before replacement with the tail, and 92.2 ms after replacement. No full allocator repair occurred. This run shared the host with the release builds. The earlier manifest 25 check measured 162.8 and 28.9 ms with the same fixture.

ChatState passed 656 tests with three ignored. The Shell storage, descendant recovery, runtime handoff, coordination, inbox and Auto permission selection passed 309 tests with seven ignored before the added active-Step test, which passed separately. The message-delivery and coordination selection passed 29 tests, including a successful idle ask triggering exactly one foreground request and active-turn and paused-Goal alternatives. The default CLI typecheck passed with the pinned Rust 1.93.1 after selecting the system `diff` on PATH. DevEco's `diff` misreported new jemalloc headers as unchanged on the default PATH.

### Child evidence and direct legacy resume

The production legacy loader returns execution state while the derived generation builds on a worker. The storage regression writes another canonical fact after that return and verifies a fresh writer sees the indexed prefix followed by the exact suffix. A finished child can leave the root's working set and still be verified from the published Seed and the exact Result in the child's short tail. The indexed read rejects a conflicting parent terminal using the full replay validator. The targeted Shell storage suite passed 437 tests with three opt-in probes excluded. The native PTY suspension probe also passed root, child and grandchild restoration across process exit and cold resume.

A direct macOS PTY open of the same 524,298,752-byte legacy fixture reported history visible at 1,975 ms and input echo at 2,095 ms. The load stage took 1,453 ms including 1,445 ms to read and decode the journal. The process exited normally before publishing the background index. The earlier synchronous candidate took 4,963 ms to echo input and 4,373 ms inside Shell load on this fixture. These are separate single runs, and the derived index was absent at the start of both. The current fixture shows the remaining cost of validating a journal that has no usable checkpoint.

Manifest 30 also passed the 515,977,893-byte publication crash probe with zero retained records on both cuts. Recovery took 623.9 ms before and 677.2 ms after manifest replacement while release compilation and other PTYs shared the host. The test checks canonical tail and original identities independently of the elapsed time.

### Background command history

Manifest 31 indexes a published background command start and its Runtime settlement. The cold projection restores the suspended runtime with zero resident events or display sources. Its page returns the start and completion at their original positions and excludes the private command output from the synthesized row. The Shell storage selection passed 438 tests with three opt-in probes excluded. The full history projector still pairs the original command starts and terminals, including the case where an ordinary completion supersedes the projected one.

The manifest 31 large crash test passed on 515,977,893 bytes with zero retained records on both cuts. The process needed 167.1 ms to recover before replacement and 29.8 ms after replacement. The same source passed 656 ChatState tests with three ignored.

The PTY harness now records the confirmed Ctrl+D exit time when it observes the child process status. Terminal output draining runs after that timestamp. Earlier exit results include EOF or a terminal quiet interval of up to 200 ms. Keep those older samples separate from new exit comparisons. A short three-point run exercised the revised harness and all 18 idle processes exited without inference or retries.

### Title and model-route history

Manifest 32 keeps the current title after 128 revisions, resolves the first revision by exact coordinate, and rejects a generated replacement after a user title. A second fixture grows the model history to 128 committed transitions plus an unpublished change. It restores the route at the suspension frontier, the latest Sampling receipt, and the final route after the tail, with no retained rows. Both fixtures use the normal writer. The Shell storage suite passed 440 tests with three opt-in probes excluded.

The 515,977,893-byte crash probe passed on this source. Recovery at the cuts before and after manifest replacement took 154.8 and 31.6 ms. Both cuts had zero retained records and preserved the same original identities and committed tail.

### Release-profile terminal comparison

The baseline is Grow 2.6.12 at `cc9ee17a1`. The candidate is the local Grow 2.6.13 build with manifest 32, SHA-256 `54751ba7ea1b91b1d32910efad685848c2e322bf839a8980a87a9af32b2c7d46`. Both product binaries use `release-dist` and Rust 1.93.1 on macOS arm64. The harness ran three paired 120×50 terminal samples per size with a local mock provider. Both writer versions wrote fixtures through their normal commit paths. Each size holds the same model-visible Surface and equivalent two-record unpublished tail, with byte equality between arms at that size. The executable paths were warmed with `--version`. Timing starts at PTY spawn. The first-operation interval starts at prompt submission after recent history has appeared. Figures are median milliseconds, baseline → candidate.

| History | First frame | Recent page | Input echo | First operation | Reopen input echo | Reopen older page |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | 69 → 65 | 155 → 167 | 169 → 168 | 211 → 215 | 114 → 117 | 292 → 299 |
| 141.64 MB | 171 → 154 | 704 → 327 | 885 → 327 | 612 → 220 | 590 → 119 | 299 → 294 |
| 515.73 MB | 158 → 134 | 1,622 → 262 | 1,983 → 248 | 1,663 → 267 | 1,793 → 128 | 287 → 293 |

First-open candidate median `session.load_light` was 25.7, 24.4 and 13.8 ms. The first recent-page read took 5.1, 5.2 and 5.2 ms. Median embedded ACP initialization before the event loop took 1.5, 22.3 and 14.5 ms. Time to the first frame and input still varied across fresh homes. The 515.73 MB case took less time than the 141.64 MB case on both first-open milestones. Reopen input was 117, 119 and 128 ms. These measurements isolate the closed Tool lifecycle and fixed suffix. Workflow, historical communication, and some descendant presentation retain a separate cold dependency.

The same candidate passed all five terminal suspension regressions, including independent processes claiming one root, completed-leaf release recovery, and root/child/grandchild restoration. Raw stage logs and casts are in `/private/tmp/grow-resume-v32-dist-ptys/`.

The separate idle Ctrl+D cohort completed successfully, with zero prompt submissions and zero retries. Baseline → candidate medians at 2.23, 141.64 and 515.73 MB were 120 → 248, 220 → 242 and 129 → 362 ms. At 515.73 MB the three candidate samples were 242, 362 and 1,181 ms. This run does not meet the exit acceptance. Storage-only crash recovery and resume isolation do not explain the exit tail. The next idle run captures the Shell suspension stages and the terminal through process exit to distinguish writer retirement from process teardown.

### Presentation Sidebands

Manifest 33 checks the separately persisted terminal of a side question, prompt suggestion, or recap before dropping its spawn from the execution fold. The regression writes 16 terminal ledgers and one incomplete ledger. The cold fold reads only the unfinished owner and persists its cancellation. The archived spawn remains queryable and duplicate spawn admission is rejected. The complete Sideband reader verifies all 17 ledgers.

The manifest 33 distribution build completed three paired PTY trials at each of the same closed-Tool sizes. The first input echo was 264 → 196, 617 → 239 and 1,808 → 178 ms. Recent history on first open was 180 → 184, 495 → 228 and 1,529 → 165 ms. The first operation at 515.73 MB was 1,588 → 200 ms. The original 1,181 ms idle exit did not recur in the separate manifest 32 repeat. Its candidate medians were 261, 235 and 237 ms. The added debug-file filters emitted no suspension timings, so those runs cannot attribute the earlier outlier. The PTY measures through observed process exit and records log draining afterward.

### Workflow epochs and SkillLoad

Manifest 34 moves Workflow transitions out of the retained replay table. The index reads the original spawn and the last boundary before the requested coordinate. The open-run cut preserves the start of each current execution epoch, including its original wall time. The Workflow list reads candidates in reverse spawn order and preserves the existing backfill rule for invalid or missing sidecars. The indexed Workflow regression restores an open epoch from its checkpoint and another from the tail, checks 20 exact lifecycles against dense replay, rejects a closed-run resume and settles the original execution epoch after interruption. The existing Workflow tests for sidecar reconstruction, cap and backfill, invalid manifests, and content identity also pass.

The SkillLoad change records pending origins on the Surface. First model materialization can identify unconsumed loads directly, and validation of a newly loaded skill resolves assistant calls by the original admission coordinate. Permission classifier and ContextRecall still use explicit branch provenance. Idle exit instrumentation records the Shell tree and node suspension, Pager suspension response and terminal restore, and CLI runtime shutdown.

The manifest 34 Shell storage suite passed 443 tests with three opt-in probes excluded, including the original closed-Turn scaling case, saved Workflow epochs, and exact SkillLoad admission plus settlement. A separate manifest 33 idle cohort produced baseline → candidate medians of 227 → 250, 224 → 273 and 531 → 458 ms. Baseline and candidate were both slower at the largest fixture in that run, which overlapped compilation. The earlier independently repeated results and those raw samples remain necessary to interpret the variance.

### Handoff receipts and retired AgentMessage batches

Manifest 35 excludes Workflow and Plan handoff receipts after settlement, indexes Workflow display updates at their original positions, and verifies the terminals of additional auxiliary Sidebands before retiring their spawn references. Its working-snapshot selection passed 30 tests with the manual large crash test excluded. That test then passed separately at 515,977,893 bytes. Both crash cases preserved the exact tail and original identities with zero retained replay records. Recovery took 208.0 ms before manifest replacement and 114.2 ms after it while compilation shared the host.

Manifest 36 moves consumed AgentMessage batches and the corresponding closed boundary dependency out of the replay fold. Its regression checks the exact original consumer, exact message identity, Surface equivalence, and pending inbox. Parent and consultation receipt evidence still enters the cold path for notice recovery and synchronization. The AgentMessage result covers the consumed batch body and its original Turn, while retaining that independent receipt evidence.

### Child interruption, recap, and startup branch boundary

The manifest 37 recovery selection passed 178 tests. The exact Tool checkpoint subprocess test separately passed before and after manifest replacement with a torn suffix. Both recovered views reported one Turn start and two Tool starts, including the historical closed call. Child recovery now uses the same indexed prefix for inspection and appends. Its aggregate usage fold preserves totals and incompleteness without opening closed usage segments.

Manifest 39 adds the published startup ContextRebuild to the bounded execution path and recent response cursor, and records recap coverage independently of historical recap payloads. The terminal fixture now includes a durable pre-turn context replacement, as an ordinary session does after startup. This supersedes the earlier fixture for release acceptance. Published rewind and image projection still require a dense branch view. Settled incoming communications, descendant presentation and Workflow sidecar backfill still need independent fixed-state growth cases.

The final manifest 39 worktree passed 444 Shell storage tests, including 31 working-snapshot tests, and 656 ChatState tests. Runtime handoff passed 13 tests with five opt-in probes excluded. The 17 message-delivery tests passed, including idle consultation continuation. The closed-Turn regression restores the same Surface bodies, item IDs, prompt and branch coordinates after one or 128 closed Turns. Both projections retain zero historical events. The response regression uses the original physical candidate position even when it precedes the context rebuild and the canonical admission follows it. A separate 515,977,893-byte test stopped the real writer immediately before and after manifest replacement. Both recovered cuts retained zero historical records. Recovery took 161.2 and 37.0 ms and verified the original suffix coordinate and exact retry.

The manifest 36 idle cohort had 18 successful launches and exits, without inference or exit retry. A → B median idle Ctrl+D was 221.5 → 236.5 ms at 2.23 MB, 222.6 → 235.3 ms at 141.64 MB and 227.3 → 358.6 ms at 515.73 MB. The slowest candidate was 664.4 ms at 141.64 MB, including 522.6 ms in Shell suspension and 425.9 ms in the node suspension. The largest candidate measured 137.9, 358.6 and 447.4 ms, with Shell suspension at 91.7, 213.8 and 390.5 ms. Terminal restore ranged from 15.6 to 115.3 ms across the candidate cohort, and process runtime shutdown was at most 3.3 ms in the samples where that span was present. These traces locate the variation in the suspension path. They do not isolate durable I/O from scheduling. Manifest 37 instruments Runtime file and directory sync separately. Raw reports are under `/private/tmp/grow-resume-v36-dist-idle-exit/`.

The operation cohort also completed all 18 A/B measurements. At 515.73 MB the first input echo fell from 3,289 to 620 ms and the first accepted operation from 2,443 to 483 ms. Compilation overlapped this cohort and first-open candidate input was 820, 883 and 620 ms across sizes. The largest candidate post-operation Ctrl+D median was 428 ms. The longest candidate exit, 2,068 ms, occurred at 2.23 MB and spent 1,642 ms in Shell tree suspension, including 376 ms in the node suspension. Terminal restore took 84 ms. This cohort does not contain a startup ContextRebuild. Raw reports are under `/private/tmp/grow-resume-v36-dist-ptys/`.

### Terminal comparison with the ordinary startup context rebuild

The manifest 39 distribution build (SHA-256 `b95e4b6403aa2886a5a62e57092aea9858536096af1d051a3122f83a675bd321`) completed the three-size mock-provider operation sweep and the separate idle Ctrl+D sweep. Each cohort completed 18 paired macOS arm64 PTY launches. Both versions wrote the same model Surface and equivalent two-record durable tail through their ordinary writer. The fixture includes the normal startup ContextRebuild before the first prompt. Each row is the median of three trials per arm in milliseconds, Grow 2.6.12 → manifest 39 candidate.

| Closed history | First frame | Recent history | First input echo | First operation | Reopen input echo | Reopen older page | Ctrl+D after operation | Idle Ctrl+D |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | 78 → 62 | 174 → 191 | 185 → 191 | 229 → 240 | 134 → 121 | 291 → 287 | 123 → 136 | 221 → 268 |
| 141.64 MB | 139 → 127 | 1,478 → 278 | 1,673 → 278 | 287 → 264 | 1,422 → 129 | 653 → 284 | 523 → 149 | 224 → 134 |
| 515.73 MB | 235 → 125 | 4,912 → 296 | 5,405 → 296 | 341 → 262 | 5,247 → 149 | 1,826 → 275 | 1,495 → 248 | 130 → 259 |

All five native PTY suspension probes passed, including concurrent claims and partial child release. The largest idle candidate samples were 242, 259 and 442 ms. The 442 ms run spent 330 ms in Shell tree suspension and 90 ms restoring the terminal. The four Runtime journal and directory barriers took 15.1, 15.0, 17.3 and 4.2 ms, and none published an index. Cold hydration decoded one retained record (259 bytes) and a 673-byte suffix in 6.3 ms. The Runtime barriers only account for 52 ms of that exit, so their traces do not establish the cause of the remaining suspension variation. The candidate largest-versus-smallest idle median did not increase. The post-operation Ctrl+D samples still vary by size and need the same stage comparison. These figures apply to closed Tool lifecycles with fixed working state. Raw traces are under `/private/tmp/grow-resume-v39-final-dist-ptys/` and `/private/tmp/grow-resume-v39-final-dist-idle-exit/`.

### Goal and operational notice paging

The standalone notice regression writes 132 retry, compaction, memory and Goal presentation rows through the normal writer. A new writer restores with no retained records or presentation sources. Two indexed pages recover the original event order and identities. The same source/revision validation runs during index publication and full replay. Current Goal authority still comes from the canonical Control snapshot.

### Outgoing coordination and image retention roots

The outgoing inquiry cut regression publishes 128 closed source inquiries and an additional source start. The suffix records the additional terminal. A cold writer restores with one resident event and zero retained rows. Exact queries return the same source audit events as dense replay, including the split terminal and a later conflicting duplicate. Ask, query and cancellation bind that lookup to their current source actor. All 64 coordination tests passed with the repository's 16 MiB test thread stack. The tool integration fixture now uses a real source Timeline in place of its no-op handle and commits the tested start and terminal before acknowledging them.

The image cleanup regression publishes an original image reference and leaves a second reference in the suffix. The pointed reads return only those references. Cancellation stops the index read, and changing the bytes of the original source prevents the cleanup lookup from confirming ownership. Dense reference extraction remains the comparison for physical messages, including pre-compaction and pre-rewind references.

An opt-in run on manifest 41 exercised process loss before and after replacing the manifest on a 515,977,893-byte journal. Both cut recoveries retained zero historical rows and verified the original authorities and tail, taking 223.3 and 47.6 ms. This run overlapped compilation. The release runner suite passed 34 tests with one skipped on this host.

The manifest 43 worktree passed all 4,822 enabled Shell library tests and 656 ChatState library tests, with 14 and 3 opt-in cases excluded respectively. The working snapshot selection passed 34 enabled tests independently.

### Manifest 43 release-profile terminal results

Both binaries used the distribution profile and Rust 1.93.1. Each size used three pairs with an independent direct-idle-exit cohort and one same-home cold re-open per operation sample. The largest fixture contained 515,734,508 bytes of journal at 30,000 closed Tool lifecycles. All fixtures had two fixed Surface items and the same semantic two-record durable suffix. The baseline retained its own publication. All sessions exited successfully without retries.

| History | First frame | Recent page | Input echo | First operation RTT | Older page on repeat | Post-operation exit | Idle exit |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | 68 → 61 | 163 → 173 | 175 → 173 | 258 → 241 | 301 → 304 | 124 → 137 | 221 → 139 |
| 141.64 MB | 68 → 70 | 1,312 → 170 | 1,466 → 170 | 267 → 207 | 678 → 295 | 486 → 135 | 224 → 233 |
| 515.73 MB | 105 → 163 | 4,622 → 356 | 4,947 → 343 | 438 → 239 | 1,698 → 285 | 1,457 → 139 | 242 → 135 |

Repeat opening moved the candidate input-echo medians to 111, 105 and 117 ms. First opening still showed a wall-clock step at the largest tier for first frame, recent page and input. The 30,000-pair samples showed post-operation candidate exit times of 137.8, 138.9 and 338.6 ms. Raw stage logs and PTY casts are in `/private/tmp/grow-resume-v43-final-dist-ptys/` and `/private/tmp/grow-resume-v43-final-dist-idle-exit/`. Manifest 44 changes communication receipts and history playback, so this is predecessor evidence.

Candidate median startup before the event loop was 33.8, 34.1 and 80.7 ms. The overlapping but later Shell load handler took 94.8, 89.4 and 171.0 ms. Within Shell, index open took 30.2, 7.1 and 9.3 ms and snapshot hydration took 0.4, 0.5 and 7.5 ms. The latter read one retained record of 257–259 bytes and a 665–673-byte tail. The recent page worker took 4.9, 5.1 and 5.9 ms. The measured step is mostly outside hydration and page IO. The first-open tests use newly written separate homes and can exercise different process and filesystem warmup; the sample does not establish an independent effect of journal size on that warmup.

The Pager library run passed 7,648 tests and caught a stale test fixture that still sent the previous descendant-ownership carrier. The fixture now supplies ownership version 3 with its completeness bit and asserts that version 2 and an invalid completeness field are rejected.

### Agent-message receipts and Workflow management

The normal-writer agent-message regression published 128 settled receipts and tested physical history anchors on both sides of publication. Indexed cold recovery retained no settled notification events. Exact replay preserved the original receipt and disposition, resolved only the requested payload root, and matched the dense history's notice bodies and event IDs across all pages.

The 515,977,893-byte process-loss regression passed on the manifest 44 Shell harness. Before and after manifest replacement, it preserved exact suffix identities and loaded zero retained records. The measured recoveries took 182.5 and 24.8 ms respectively. The fixture was built through the production storage adapter.

Workflow recovery checks passed for the published execution epoch, execution-suspended boundaries, the original latest-128 backfill, exact manifest ownership, on-demand management, and merge into an existing live tracker. The indexed cold test gives an old closed candidate an invalid sidecar path. Writer startup succeeds and explicit history management observes the error. The first full Shell pass exposed two outdated workflow test fixtures and a read-only path that no longer enforced the historical cap. The read-only load retains the full historical view, and both in-memory persistence fixtures acknowledge the new management read. All four targeted Shell Workflow cases and both ChatState cases passed in the follow-up builds.

The consultation regression publishes seven settled answers and an eighth answer whose receipt is still pending. Recovery retains only the eighth terminal, still resolves the first inquiry's exact start and terminal, and validates the receipt appended in the next suffix. The selected working Surface remains authoritative. The earlier InfoRequest ledgers stay available to complete validation.

The manifest 46 snapshot suite passed 37 enabled cases. It includes the consultation, communication and the presentation retry regression. The retry test commits a retained assistant row, verifies that the same ID and payload leave the journal length unchanged before and after reopening, and rejects the same ID with changed bytes.

### Inherited history, descendant ownership, and remaining branch cases

The inherited-text regression publishes 129 child-local seed display references, replaces the Surface, and reopens through the indexed writer. The cold history has no resident presentations and preserves the published display count. Requested pages recover the original coordinates, text, and metadata. The first inherited assistant message is over 8 MB when encoded. The test reconstructs it from byte continuations with no provider admission identity on any fragment. The response index seeks by the latest ContextRebuild partition and physical anchor, so older rebuild partitions do not require filtering a range of obsolete response rows.

A manifest 47 test process writes a 515,977,893-byte journal and stops on each side of manifest replacement. Recovery retains zero historical records in both cases, verifies the original domain facts and suffix, and takes 151.7 ms before and 28.1 ms after publication. These are storage measurements, separate from PTY milestones.

All five native PTY suspension probes passed on the first manifest 47 distribution build, alongside all ten independent-process coordination checks. The subsequent Pager library run passed 7,650 enabled tests. The Shell descendant recovery module passed all ten tests, including settled direct child ownership resolved from its exact displayed identity and ordered nested lifecycle replay.

The next manifest 47 release candidate completed all 18 direct idle Ctrl+D trials with no inference, exit failure or retry. Baseline → candidate medians were 221 → 241 ms at 2.23 MB, 124 → 241 ms at 141.64 MB and 133 → 240 ms at 515.73 MB. The three largest candidate observations were 240, 240 and 243 ms. This build predates the unpublished ContextRebuild fix and recall sealing. Raw samples are under `/private/tmp/grow-resume-v47-locked-dist-idle-exit/`.

The same candidate (SHA-256 `6ccb8106b2bffecf973fa74aaca262455b0734925d20ecc57b5a54d566424a75`) completed the paired operation cohort. Both binaries used the distribution profile on macOS arm64. Across all three trials per arm and per size, the same two-item Surface and two unpublished durable records were verified. Figures below are median milliseconds, baseline v2.6.12 → manifest 47.

| History | First frame | Recent page | Input ready | Actual input echo | First operation RTT | Older page on reopen | Post-operation Ctrl+D | Idle Ctrl+D |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | 78 → 70 | 155 → 220 | 155 → 208 | 168 → 220 | 236 → 233 | 301 → 294 | 222 → 174 | 221 → 241 |
| 141.64 MB | 152 → 134 | 1,540 → 251 | 1,686 → 240 | 1,698 → 251 | 287 → 221 | 665 → 299 | 126 → 135 | 124 → 241 |
| 515.73 MB | 147 → 213 | 4,591 → 352 | 5,002 → 342 | 5,015 → 366 | 440 → 329 | 1,710 → 305 | 1,481 → 264 | 133 → 240 |

Candidate first-open median time before the event loop was 33.1, 60.2 and 89.9 ms. Shell load was 27.7, 13.3 and 14.8 ms. Each cut read one retained row of 257–259 bytes, plus a 665–673-byte tail. Hydration took 0.5, 6.1 and 6.4 ms and the history page read took 5.0, 5.2 and 5.4 ms. The time difference within these storage stages does not account for most of the first-open wall-clock spread. The larger candidate was substantially faster than the baseline, while its first-open frame, input and operation medians still exceeded those for the small candidate fixture. Raw samples and traces are under `/private/tmp/grow-resume-v47-locked-dist-ptys/`.

An all-Shell run during release compilation exposed two unrelated timing-sensitive assertions. The file-metadata watcher passed its isolated rerun. The native child control test used the parent usage-commit request as a child Turn completion barrier. The prompt reply can reach parent settlement before the child emits its terminal. The fixture now waits for the original child's `TurnCompleted` in addition to blocking parent usage, then verifies the delayed model request cannot mutate the retired child. The corrected isolated test passed.

The startup ContextRebuild test now appends its replacement after an existing publication, reopens with the published index, and restores the original Surface identities and the updated branch. The startup change keeps inherited text across the new branch while selecting provider admissions through that branch's index.

ContextRecall now uses the same frozen-coordinate checks at live materialization and full replay. Publication retires a completed or interrupted sealed ledger after checking its exact prefix digest and the original source cut. The Shell regression recovers only the open and legacy unsealed ledgers at the first cut, then only the legacy ledger at the next. It verifies all 20 ledgers through the complete reader. Fourteen ChatState Sideband tests passed, including rejection of changed sealed payload.

A pre-existing rewind or image projection still needs the dense branch projector. The indexed descendant reader moves tree traversal off the Pager reducer and resolves replayed direct-child ownership at the captured cut. This does not establish bounded child transcript reads for every subtree size.

### Manifest 48 storage and terminal verification

The complete Shell run passed 4,829 enabled tests with 14 opt-in tests excluded. This includes the original 5-to-513 closed Turn/Step regression, the archived child lifecycle and terminal-only root page, suspended ownership, explicit rewind across compaction, and progress steering with the completion gate still in effect on the real final reply. The large publication test also passed separately with 515,977,893 bytes. Both subprocesses stopped at the expected manifest boundary. Recovery took 149.9 ms before replacement and 25.8 ms after replacement. Both cuts retained zero replay records and preserved the original suffix sequence and exact retry.

The next terminal comparison prepared all independent fixtures before timing any launch. Both binaries were warmed afterward. The sample order, ordinary writer, fixed model Surface, and two durable unpublished records were preserved. Candidate first-frame medians were 80, 85, and 82 ms. Recent history medians were 214, 174, and 195 ms. Input echo medians were 225, 174, and 208 ms. Operation roundtrips were 231, 218, and 204 ms. Direct idle Ctrl+D medians were 137, 134, and 135 ms. Both cohorts completed all 18 first-open pair samples each through 515.73 MB. The separated fixture phase removed the size trend seen when each fixture was written immediately before the corresponding timed launch. It does not identify a single cause for every previous outlier. The Pager terminal-only routing correction followed this build and its verification is separate. Raw results are under `/private/tmp/grow-resume-v48-final-dist-ptys/` and `/private/tmp/grow-resume-v48-final-dist-idle-exit/`.

### Final release-profile verification

Pager passed 7,651 enabled tests (15 opt-in), including the terminal-only child route after the root has finished replay. Canonical spawn ownership creates the child view without adding a duplicate Started row or invoking synchronous inherited-history reads. ChatState passed 660 enabled tests (three opt-in). The Shell run and 515.98 MB crash comparison above use the same storage source as the final candidate.

Rust 1.93.1 built both macOS arm64 binaries with `release-dist`. The baseline v2.6.12 SHA-256 was `988435fa8c1c9c62322c8c78c2e2b6d1f375a2de861885e793d843f900f245ad`. The final candidate including the Pager correction was `78e35740712e8111a53147cad1b289e9054af7aebfe945c78369dae516edada9`. Its build-time version string identifies the branch base because the source had not been committed. The official build will stamp the release tag's exact commit. Both cohorts completed 18 launches with successful exit, plus a repeat open for each operation sample. Both arms used their own production writer. They matched the two-item model Surface across sizes and the two durable unpublished records byte for byte for each size. Measurements are medians of three samples per arm in milliseconds, A → B.

| Journal | First frame | Recent page | Execution ready | Input echo | First operation RTT | Older page on repeat | Ctrl+D after operation | Idle Ctrl+D |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2.23 MB | 67 → 63 | 153 → 167 | 153 → 156 | 166 → 167 | 198 → 180 | 308 → 300 | 121 → 139 | 221 → 133 |
| 141.64 MB | 68 → 67 | 1,404 → 170 | 1,467 → 170 | 1,480 → 187 | 217 → 193 | 683 → 295 | 127 → 137 | 122 → 133 |
| 515.73 MB | 66 → 67 | 4,696 → 164 | 5,023 → 164 | 5,037 → 177 | 292 → 217 | 1,769 → 305 | 1,549 → 137 | 130 → 133 |

The largest fixture held 30,000 closed Tool pairs and at least 515,734,508 committed bytes. Candidate startup before the event loop took 35.6, 35.6 and 35.5 ms. Indexed hydration read one retained record of 257–259 bytes and the 665–673-byte tail at all three tiers. Median hydration was 0.40, 0.53 and 0.48 ms. Reading the recent page took 5.00, 4.91 and 5.08 ms. The final run did not reproduce the earlier first-open startup step. First-operation medians increased by 37 ms from the smallest to largest fixture, with overlapping individual ranges of 178–226 and 188–221 ms. The previous clean run gave 231 and 204 ms at those tiers. Together with the fixed executed read work, these measurements support bounded scaling for the tested lifecycle. They do not establish a flat latency guarantee. The largest direct idle exits were 132.5, 133.1 and 133.6 ms. A middle-tier sample took 232.6 ms. Raw stage logs, casts and full observations are under `/private/tmp/grow-resume-v48-release-dist-ptys/` and `/private/tmp/grow-resume-v48-release-dist-idle-exit/`.

The final candidate also passed all five opt-in native PTY nested suspension/release regressions and all ten independent-process local coordination probes. The official release workflow source, platform catalog, lockfile versions, notes and shell/YAML syntax passed local preflight.
