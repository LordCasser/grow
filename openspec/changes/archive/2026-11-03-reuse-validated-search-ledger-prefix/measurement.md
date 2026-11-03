# S2 read projection measurement

## Timing method

Command:

```sh
GROW_HOME=/tmp/grow-backlog-s2-home RUST_MIN_STACK=16777216 \
  cargo test --locked -p shell --lib \
  session::storage::search::tests::search_projection_timing_probe \
  -- --ignored --exact --nocapture --test-threads=1
```

Raw command output: `/tmp/grow-search-projection-timing.txt`. The ignored probe source is preserved in [`probe.rs`](probe.rs); it was run in one test process with 2 warmups and 7 measured samples per case. It prepares one `TempDir` fixture for each size. The old full collector and the projection implementation were measured in the same executable against the same baseline bytes; these are separate reader instances, not separate before/after binaries. Both include reader open, full-content projection, and extraction. The unchanged case first builds one projection, then reuses it in the same process and fixture. Append and rewrite setup, including baseline checkpoint construction and fixture writes, is outside the timer; their measured section opens a reader, proves/scans the prefix, folds as needed, and extracts complete content. All cases assert complete extracted content equality against their expected Timeline.

The fixture is synthetic, warm filesystem/debug test profile, and measures the reader plus content projection kernel. It does not include Tokio scheduling or SQLite upsert. Payload bytes are serialized JSONL file length; they are not retained heap estimates. For the old full collector, its returned `Timeline` is dropped inside the measured function. For cold/append/rewrite projection cases, the measured interval ends before the returned projection is dropped; unchanged intentionally retains it. This reflects the ownership path, but means the timings are not an isolated decode-only comparison. Prefix proof still hashes the full old prefix and extraction still walks the complete selected Timeline, so unchanged/append gains do not imply O(1) work. The largest fixture is 3.17 MB and differs from a previously recorded 4.92 MB debug fixture; do not compare those absolute times as if they were one benchmark series.

| Turns | Serialized payload bytes | Old full median | Cold projection median | Unchanged median | Append one turn median | Same-size rewrite fallback median |
|---:|---:|---:|---:|---:|---:|---:|
| 128 | 196,153 | 18.96 ms | 20.63 ms | 3.08 ms | 3.24 ms | 22.77 ms |
| 512 | 788,853 | 78.67 ms | 78.18 ms | 11.85 ms | 11.92 ms | 85.58 ms |
| 2,048 | 3,169,239 | 313.26 ms | 311.40 ms | 46.57 ms | 47.10 ms | 347.67 ms |

The cold projection is approximately the old full path because it validates and decodes the full file. A same-size rewrite falls back to full rebuild after hashing and costs more than a cold projection. Unchanged and one-turn append cases retain full-content extraction and prefix hashing but avoid re-decoding and re-folding prior events.

## Raw samples (nanoseconds)

```text
128 old_full         median=18959750 samples=[18957042,20592875,23409417,18959750,18841959,18950333,19719875]
128 cold_projection  median=20626417 samples=[20861000,21376250,20626417,19779125,20126042,20727833,20384458]
128 unchanged        median=3079417  samples=[3012084,3109209,2980375,3079417,3078666,3083250,3171334]
128 append_one_turn  median=3241333  samples=[3160458,3190792,3372542,3374417,3241333,3339584,2972792]
128 rewrite_fallback median=22767000 samples=[23043750,22729250,22928500,22627792,23097666,22767000,22634875]
512 old_full         median=78667250 samples=[81830458,81060834,78292708,78792250,78614583,78362333,78667250]
512 cold_projection  median=78179958 samples=[78699375,78832500,78256417,77755834,77596167,78179958,77674833]
512 unchanged        median=11846875 samples=[11863416,11799042,11829917,11846875,11899291,11939041,11595750]
512 append_one_turn  median=11917709 samples=[11909792,11870500,11776042,11917709,12122625,12099917,12037000]
512 rewrite_fallback median=85580000 samples=[85533959,85964709,85580000,86199083,85059666,85437209,85765625]
2048 old_full        median=313262500 samples=[313950833,313545625,312015375,313463375,312883833,313262500,312400042]
2048 cold_projection median=311398083 samples=[307251750,310785208,316588250,324284000,312065625,311069042,311398083]
2048 unchanged       median=46570209 samples=[46432667,46570209,46229959,46718666,46211375,46594500,46768875]
2048 append_one_turn median=47104500 samples=[46956167,47104500,47758667,60126458,46827375,47387125,46991625]
2048 rewrite_fallback median=347667666 samples=[347157291,348405833,346893916,347449625,348672667,350894125,347667666]
```

## Retained allocation measurement

Command:

```sh
GROW_HOME=/tmp/grow-backlog-s2-home RUST_MIN_STACK=16777216 \
  cargo test --locked -p shell --lib \
  session::storage::search::tests::search_projection_retained_allocation_probe \
  -- --ignored --exact --nocapture --test-threads=1
```

Raw output: `/tmp/grow-search-projection-memory.txt`; probe source: [`memory-probe.rs`](memory-probe.rs). The fixture bytes and source `Timeline` are live before the baseline. One projection is warmed and dropped first; then the probe records live allocation bytes, constructs a fresh projection, samples live bytes, drops it, and samples again. It runs in one test process with one test thread while the temporary global `CountingAlloc` is enabled. The live delta returns exactly to baseline for these fixtures.

| Turns | Serialized payload bytes | Domain timeline events | Physical journal records | Retained requested live allocation bytes | Released on drop |
|---:|---:|---:|---:|---:|---:|
| 128 | 196,153 | 512 | 512 | 262,776 | 262,776 |
| 512 | 788,853 | 2,048 | 2,048 | 1,050,816 | 1,050,816 |
| 2,048 | 3,169,239 | 8,192 | 8,192 | 4,205,664 | 4,205,664 |

The allocator counts requested Rust allocation layout sizes while allocated; it is not malloc usable size, allocator fragmentation, or process RSS. Serialized JSONL payload, event/record counts, live allocated bytes, and RSS are separate quantities. No `size_of::<TimelineReadProjection>()` value is treated as retained heap. This is a synthetic fixture with no presentation records, so the Timeline event count equals physical records and therefore the scanner's `AppendState.events` and `.records` counts. Live allocation bytes include both Timeline and `AppendState` allocations.
