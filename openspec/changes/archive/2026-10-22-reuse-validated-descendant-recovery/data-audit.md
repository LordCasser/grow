# Independent data audit

Read-only review by `/root/c3_typed_grow_facts`, after formal measurement completed; no extra tests or benchmarks run and no implementation edits. This is an independent numeric/evidence audit, not a second independent architecture review. Root self-adversarial code review is in `adversarial-review.md`.

Verified default-off `metric_type=resume-pty` ordinary2048 ready800.1 [789.2,801.4]→577.1 [576.4,610.8]ms, last550.3 [548.8,554.5]→548.2 [546.8,553.6]; slow2048 ready873.2 [854.5,900.2]→667.1 [662.6,697.1], last690.3 [683.6,854.5]→667.0 [662.6,668.0]. `resume-progress` and detailed-log modes must remain separate.

Verified root loading-prefix read/decode counts[2,2,2]→[1,1,1], durations403.734→201.506ms; finalize226.287 [214.320,233.111]→3.827 [2.855,4.013]ms. These are not all-process counts: slow raw process summaries may be3→2. Backend RPC independently has its original one primary read in both versions.

Verified all four child benchmark groups n=3, excluding per-stage/shape warmups (16 raw rows,12 timed). Star169.767 [163.427,170.114]→90.835 [90.700,90.965]; chain169.405 [165.279,170.553]→90.789 [90.772,97.843].

Verified14 scoped groups sum2522 passed/0failed/7ignored, one explicit release-only debug-assert exclusion; four distinct manual PTY passes. Overlapping new checks/warmups/bench invocations are not added to unit counts. RPC12 complete oracle runs (two profiles×two versions×three repeats), one passed test per run; modern2048 has4096 events,67,191,722 historical body bytes,74,843,515 Timeline bytes,updates0. File identities match the corresponding retained executables and per-run identities. Root separately recomputed all10 retained executable SHAs against the before/after indexes and confirmed final runtime/source hashes unchanged after measurement.
