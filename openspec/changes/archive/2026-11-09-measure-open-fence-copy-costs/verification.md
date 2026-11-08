# Open-fence cost measurement

## Source and execution

Rust/Cargo 1.93.1 on Linux x86_64. The integrated source tree at `c9e8840b` and this change have the same `open_fence_highlighter.rs` Git blob, `6d29a96689c384737dafd9c4fa67053838f2008f`.

- `cargo test --locked --release -p markdown --lib -- --test-threads=4`: **502 passed, 0 failed, 1 ignored**, exit 0. The ignored item is this explicitly selected measurement.
- `cargo test --locked --release -p markdown --lib open_fence_highlighter::tests::measure_open_fence_parse_and_copy_costs -- --ignored --exact --nocapture --test-threads=1`: **1 passed, 0 failed, 502 filtered out**, exit 0, 12.89 seconds test runtime in the final run.
- The final measurement ran without a concurrent build. An earlier passing run overlapped Clippy and is not the timing reference below. Optimized release code was used; no debug timing is presented as production performance.
- Styled highlighter output equals fresh batch highlighting. Every sampled real `StreamingMarkdownRenderer` growth frame matches batch styled lines and source maps. Existing Markdown regression tests also pass.

## Final observations

All times below are milliseconds. The fixture grows a YAML fence by one complete line per call. Parse and style/output phases are independent mechanism measurements using the same syntect operations as production; they are not obtained by subtracting different end-to-end runs. Warmed clone and prefix timings are means over 16 iterations. Real final-growth frame timings use eight separately primed renderers.

| Lines | Source bytes | Cumulative returned text bytes | All incremental calls | Parse total | Style/output total | Warmed output clone | Prefix check | Growth frame median | Growth frame max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 256 | 11,264 | 1,447,424 | 65.100 | 21.066 | 5.578 | 0.140 | 0.009 | 0.631 | 0.790 |
| 1,024 | 45,056 | 23,091,200 | 475.560 | 80.460 | 23.688 | 0.560 | 0.040 | 2.034 | 2.256 |
| 4,096 | 180,224 | 369,188,864 | 6,195.770 | 314.887 | 92.445 | 3.101 | 0.167 | 7.772 | 8.638 |

`returned_text_bytes` counts logical source text copied into the owned return for every growing prefix. It is not allocated capacity, peak memory, RSS or total downstream allocations. For this fixed 44-byte-per-line input the exact sum is `44 * N * (N + 1) / 2`. Parsing each new line remains incremental, but returning all prior owned text retains the quadratic cumulative copy cost.

## Interpretation and decision boundary

- MD-1 is now measured rather than only inferred from source. At 4,096 lines, a warm output clone costs 3.101 ms and the real Markdown growth-frame median is 7.772 ms. The cloning residual is not repaired by this test.
- The default animation configuration is 30 FPS (`pager-render/src/appearance/config.rs`), about 33.3 ms per animation interval. These isolated renderer samples do not exceed that interval, including the 8.638 ms maximum sampled frame. This is a narrow observation, not a whole-UI frame-budget pass: terminal drawing, layout, other messages, higher refresh settings and other hardware are absent.
- The clone and renderer timings were measured separately and must not be treated as additive cost accounting or a precise percentage of a frame. Both increase with fence length; no claim is made that copying has constant cost or no measurable effect.
- No production representation change is justified solely by this one synthetic workload. Retain MD-1 as measured performance debt. Any follow-up optimization should address the existing highlighter/output owner, preserve batch styled output/source maps, and demonstrate that committed prefixes are reused rather than merely reducing syntect parsing again.
- No new production API, cache policy, frame limit or timing assertion is introduced. The ignored fixture is repeatable evidence, not an unstable performance gate.

## Validation limits

This covers one newline-terminated YAML workload with complete-line appends. It does not measure partial-token arrival, every language, nested Markdown, table/URL complexity, terminal I/O or native macOS/Windows timing. The 502-test suite provides broader output regression coverage, not platform performance evidence. Remote CI remains the integration owner's responsibility; this local result is not a claim that the whole repository is green.

Documentation validation on the isolated worktree: OpenSpec strict **23/23 passed**, all touched relative links resolved, and `git diff --check` passed. Integration has additional independent changes and must validate its own combined tree.

## Archive boundary

`openspec archive measure-open-fence-copy-costs --yes --skip-specs` confirmed all measurement tasks complete and archived only this change under `2026-11-09-measure-open-fence-copy-costs`; no main behavior specs were updated. Backlog and maintenance-disposition links now resolve to this archive.

Post-archive checks on the isolated worktree passed: all strict **22/22**, archived strict **924/924**, touched relative links and `git diff --check`. Archiving completes the measurement deliverable only. MD-1's copying debt remains in backlog, the separate maintenance change stays active for its pending real prompt runtime check, and ACP's explicit deferral is unchanged.
