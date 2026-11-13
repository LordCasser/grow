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
