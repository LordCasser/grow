# Verification
- main; low-disk Cargo flags: incremental off, dev/test debug off, jobs 2, RUST_MIN_STACK=16777216; locked/offline shell --lib tests.
- image_description_deadline_tests: 3 passed, 0 failed, 0 ignored, 0.00 s. Paused clock verifies no provider poll after preparation exhausts budget, partial preparation preserves original deadline, and timely completion succeeds.
- image_input_recovery_tests first run aborted with stack overflow on explicit 8 MiB test thread. Increased test-only stack to 32 MiB; rerun 3 passed, 0 failed, 0 ignored, 0.41 s. Covers missing auxiliary model, auxiliary rejection preserving images, and active Goal successful description/retry.
- No full actor-level delayed-persistence fault injection, real provider/network timeout, Windows run or hard end-to-end filesystem deadline proof. No production stack setting changed; pre-change stack behavior was not measured.
- Linker emitted large __eh_frame compact-unwind warning; tests completed successfully after fixture adjustment.
- Strict validation: all 16 passed; archived 229 passed. cargo clean removed 7,372 files / 2.7 GiB; final available disk 66 GiB.
