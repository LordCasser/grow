# Old selector baseline and differential plan

Successful baseline output is `/tmp/grow-fsnotify-t1-before.txt`; `/tmp/grow-fsnotify-t1-before-path.txt` points to the raw log. The initial filter invocation accidentally selected zero tests and is not the baseline: the logged successful run used the test-name substring without `--exact` and ran one ignored probe.

Command:

```sh
GROW_HOME=/tmp/grow-home-fsnotify-t1 RUST_MIN_STACK=16777216 cargo test --locked -p fsnotify --lib per_dir_selection_before_baseline -- --ignored --nocapture
```

The full filesystem walk created 120 groups × 99 leaves plus the 120 group directories (12,000 total candidates) before timing. The walker uses the production `pruning_walker` and `select_per_dir_watch_dirs` with no ignore rules; each sample includes traversal, collection, and the existing stable depth sort, but no OS watch registration. It returned 12,000 candidates. Two warmups and seven raw samples gave median **314.60 ms**. The original raw log is retained in [measurement-before.txt](measurement-before.txt).

The original before matrix used synthetic `PathBuf`s with six interleaved depths and the production stable sort/truncate. The final after probes use a typed semantic oracle that precomputes each `(depth, path)` once, then stable-sorts by depth and truncates. This preserves the old selected order, including equal-depth source order, but does not reproduce the old comparator's repeated `path.components().count()` cost. The original 314.60 ms full walker measurement above is an independent run of the before production implementation; it is not the paired reference used by the after probes.

The fixture patterns are `Mixed`, `Equal`, `CyclicUp` (`ordinal % 6`), and `CyclicDown` (`6 - ordinal % 6`). The last two are cyclic depth patterns, not globally ascending/descending sequences. N = 0, 1, 128, 512, 2,048, 12,000; budgets are the deduplicated values from 0, 1, 8, 128, N, and 49,152. The final timed probe matrix measures Mixed and Equal; the permanent differential test also covers both cyclic patterns.

The bounded-selector differential test feeds identical candidate sequences to the stable-sort oracle and heap selector, then compares exact `Vec<PathBuf>` order and total candidate count. It covers mixed/equal/cyclic-up/cyclic-down depths; N=0/1/128/512/2048/12000; budgets 0/1/8/128/N/MAX. The cyclic patterns use `ordinal % 6` and `6 - ordinal % 6`; they are not globally ascending/descending. A reversed equal-depth candidate vector also checks that input order remains the tie-breaker when it is opposite lexical path order, at B=8 and default B. B=0 reports N and retains no paths; B>=N returns the complete stable order. Existing filesystem tests still cover pruning, custom globs, root exclusion and symlink handling. An actual 12,000-directory filesystem differential also compares B=8 and the default budget.

## First heap-only selector measurement

The first after-probe compared the old stable sort/truncate with a heap selector that always used `BinaryHeap`. It is retained as a prototype diagnostic in [measurement-after.txt](measurement-after.txt), with source in [probe-after.rs](probe-after.rs). The final implementation avoids the heap when the candidate count fits the budget; see the final measurements below.

## Final budget-aware selector measurement

The final probe uses the same typed old-reference comparison, fixture shapes, and 12,000-directory filesystem as the first probe. It retains [measurement-budget-aware.txt](measurement-budget-aware.txt); source snapshot is in [probe-budget-aware.rs](probe-budget-aware.rs). Each probe ran both old reference and new candidate in the same test process; the first and final probes were separate test processes on the same host. Each matrix case used two warmups and seven raw samples.

The matrix's historical raw label `pure` is inaccurate: `timing_samples` measures the full `run` closure. For owned-input cases, each closure clones the candidate input before its inner timer and drops the returned path vector before returning, so raw samples include input clone, selection, output drop, and timer/black-box overhead. Equality checks and fixture construction are outside timing. Treat these as owned-input selection timings, not clone-free selector timings. The actual filesystem case creates its tree before sampling; each closure times the full walker plus selection and returned-path drop, with no OS watch registration.

At N=12,000 and default B=49,152, the final no-overflow path retains all candidates and stable-sorts by depth. Its owned-input timing median was 1.63 ms for mixed depths and 0.835 ms for equal depths; the typed stable-sort reference medians were 1.28 ms and 0.601 ms. These are approximately 27% and 39% timing regressions for this probe. At B=8 with N=12,000, mixed-depth heap selection was 0.850 ms versus 1.12 ms reference; equal-depth was 0.775 ms versus 0.456 ms reference. These are owned-input timings, not an end-to-end watcher or a clone-free selector measurement.

On the real 12,000-directory tree, B=8 full-walk medians were 307.2 ms new and 300.6 ms old; default B=49,152 medians were 302.2 ms new and 301.0 ms old. The default is not truncating at N=12,000. The default full-walk difference is about 0.4%; this includes filesystem enumeration, candidate path creation, and selection, with no OS watch registration. These measurements do not show reduced filesystem I/O.

The selector's retained candidate count is at most B; when N<=B it retains N, and B=0 still traverses/counts all candidates with no retained entries. This is a count bound, not an exact allocation-capacity promise: `Vec` growth may reserve a few extra slots. The storage is O(min(N,B)); no allocated-byte measurement was run.

The source-only pre-change `watcher.rs` snapshot is at `/tmp/grow-t1-source-before-watcher-path.txt` (currently resolves to `/tmp/grow-t1-source-before.vPMHxe/watcher.rs`).

The baseline probe source is preserved in `probe.rs`. The first and final post-change ignored probes are preserved in `probe-after.rs` and `probe-budget-aware.rs`; their timing helpers have been removed from the build after measurement.
