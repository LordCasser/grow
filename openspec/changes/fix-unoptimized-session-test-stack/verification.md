# Verification
Before: isolated test aborts with stack overflow; default RUST_MIN_STACK does not override its explicit 8 MiB Builder stack. LLDB confirms production poll stack allocation rather than test concurrency.

Diagnostic experiment: boxing only the three external call sites still overflowed in the same production body poll frame. The large body remained embedded in tracing-generated inner futures. These ineffective call-site edits were reverted before moving ownership inside the instrumented boundary.

A second experiment boxing the traced loop body also overflowed. Measured future sizes (bytes): model switch 131760; background 131496; image projection 129488; tool loop 32104; loop body 136272. Production edits and temporary size instrumentation reverted. The next validation adjusts only test-stack capacity, with existing assertions and production stack unchanged.

Compaction suite with unchanged assertions and 32 MiB test stack: 24/24 passed. Turn pipeline suite still passes 13/13 on its original 8 MiB stack, so it remains unchanged. Stop/cancel suite independently overflows on its original 8 MiB worker before its first assertion sequence; align that harness too.

Full Shell suite after test-stack adjustments: 3765 passed, 3 ignored, no stack overflow. One separate reaper lifecycle test failed under concurrency and passes in isolation; it is tracked for a separate fix. The full suite includes 24 compaction, 13 turn-pipeline and 2 stop/cancel tests passing. Production turn/mod.rs has no diff and production SESSION_THREAD_STACK_SIZE remains 8 MiB.
