# Independent review

Result: no remaining production or evidence blocker found after the documentation corrections and anti-lexical test strengthening. This was a static review against the source-only watcher.rs snapshot named by `/tmp/grow-t1-source-before-watcher-path.txt`, followed by review of final tests, preserved probes and raw samples. The reviewer did not run Cargo or edit production/tests.

## Selection and ownership

- The production pruning walker remains lazy through filter_map into the selector. Root exclusion, directory type checks, symlink handling, VCS/custom pruning and the existing flatten/error behavior are unchanged. Depth still means path.components().count(). No whole candidate collection precedes the budget boundary.
- At most B candidates are initially collected with take(budget); the implementation neither computes budget+1 nor eagerly reserves budget slots. If the stream fits, stable depth-only sorting retains original source order. Otherwise BinaryHeap::from reuses the collected candidates, and strictly smaller (depth, ordinal) keys replace the largest retained key. Ordinals are unique, so PathBuf ordering cannot alter equal-depth ties. Sorted heap output equals the original stable depth-sort/truncate result.
- Overflow processing consumes the entire remaining stream and reports the full usize candidate count. B=0 still counts all candidates while retaining none. Startup warning uses this complete count, not selected.len(). Root/Git watch layout, later registration failure handling, Update/Reconcile and configuration defaults are unchanged.
- The selection's retained count is bounded by B, with O(min(N,B)) candidate storage, not an exact allocated-slot or heap-byte guarantee. Standard Vec growth and stable-sort scratch space can reserve additional slots. Walker state and transient current entries are distinct from retained selection storage. When N<=B, including the measured 12,000/default-budget case, all N candidates remain retained; that case does not demonstrate lower candidate retention.

## Differential coverage

The typed oracle independently stable-sorts/truncates the same candidate sequence. Its cached depth preserves the old selection semantics, though it is not a verbatim copy of the old comparator's repeated components().count() work. Permanent tests compare exact path order, selected length and complete count across empty/single/small/12,000 inputs, mixed/equal/cyclic-up/cyclic-down depth patterns and budget boundaries. Existing real-filesystem tests keep pruning/glob/symlink/root behavior, and the actual 12,000-directory fixture compares both B=8 and the default budget.

Review found that the original synthetic path names increased with input ordinal, allowing a mistaken lexical tie-breaker to pass. The final test reverses equal-depth input and checks B=8 plus the default budget, covering both overflow and no-overflow paths. The owner reran that exact strengthened test successfully. No new product algorithm was required.

## Measurement interpretation

All reported medians were checked against the preserved samples: 34 values in the original before log, 136 in the heap-only log and 136 in the final budget-aware log agree with their raw arrays. The final raw series and prototype series are kept separate; the original production-selector 314.60 ms result is an independent run, not the paired typed-reference comparison.

Review identified a timing-description error: the outer timing_samples wrapper measures the entire run closure even though that closure also computes an inner duration. The recorded owned-input samples therefore include candidate cloning, selection, returned-path drop and timer/black-box overhead. Fixture construction and equality checks are outside timing. Documentation now states that boundary and calls the paired old path a typed semantic reference. Its filesystem wrapper also computes cached depths before sorting, so it is not a verbatim old production-path timing. The final default-budget regressions and the B=8 equal-depth regression are retained without claiming universal speedups.

The actual filesystem measurements include enumeration and returned-path drop, not OS watch registration. Both before and after still traverse all directories; neither reduced I/O nor fewer kernel watches relative to the previous selected set is established. No allocated-byte or RSS probe was run. The supported improvement is budget-bounded candidate retention when N>B. Default N=12,000/B=49,152 retains all candidates and measured roughly 0.4% slower full-walker time against the paired reference; this is not a product SLA.

The build contains no temporary timing probe. Preserved probe headers now avoid duplicate helper/test insertion. The heap-only caller snapshot explicitly requires the historical heap-only selector to reproduce its original raw series; pasting it into current code only recreates that caller shape. The final budget-aware probe can use the current selector. This distinction resolves the source/version reproducibility concern without inventing new measurements.

## Owner-executed validation

The owner reports the post-probe-removal `cargo test --locked -p fsnotify` results: library 119 passed/15 ignored, integration 2 passed/6 ignored, doc tests 0. The two differential tests passed, and after adding the reversed-order assertions the exact boundary test passed again. That final test-only strengthening was targeted-rerun; the earlier full-package run is not represented as containing those later assertions. Strict OpenSpec validation passed 18/18 and scoped diff checks passed. These commands were executed by the implementation owner with isolated GROW_HOME and RUST_MIN_STACK=16777216, not by the reviewer.
