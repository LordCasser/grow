# Verification
- main, locked/offline shell --lib tests with incremental/dev-debug/test-debug disabled, jobs 2 and RUST_MIN_STACK=16777216.
- completed_image_sideband_is_recoverable_before_parent_projection: 1 passed, 0 failed, 0 ignored, 0.15 s. Extended fixture queries valid/duplicate, wrong prompt, wrong SurfaceId and wrong revision; verifies newer completed spawn selection and that an unrelated SessionTitle corrupt ledger fails the batch. Empty-query lookup skips even that corrupt history.
- image_input_recovery_tests: 3 passed, 0 failed, 0 ignored, 0.44 s (existing 32 MiB test thread).
- image_description_deadline_tests: 3 passed, 0 failed, 0 ignored, 0.00 s.
- One-load property verified structurally: actor calls batch lookup once outside provider loop, only if uncached queries exist; storage loads/folds/validates before iterating queries. No syscall-count instrumentation or latency benchmark. Candidate matching still scans history per query; this change removes repeated disk reads/folds/validation, not all multiplicative CPU work.
- Blocking operation returns only matching output/provenance; historical ledgers drop before return. No new persistent index, no production-user-state mutation and no Windows or concurrent append snapshot test. Existing filesystem/total-history size bounds are unchanged.
- Linker reports existing large __eh_frame compact-unwind warning; all selected tests pass.
- Strict validation: all 16 and archive 231 passed. cargo clean removed 7,372 files / 2.7 GiB; available disk 65 GiB.
