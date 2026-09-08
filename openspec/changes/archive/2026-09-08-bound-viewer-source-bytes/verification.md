## Verification

`cargo test --locked --offline -p pager-render --lib prompt_images --quiet`:165passed,0failed,0ignored (0.08s). Build used disabled incremental/debug info, two jobs and RUST_MIN_STACK=16777216.

New tests exercise the actual loader implementation with small injected allowances and the same real PNG in memory and file form: exact fit preserves bytes/dimensions, zero and one-byte-short budgets reject, and empty memory fails. The public wrapper supplies50,000,000 bytes. Memory to_vec is inside the admitted length branch.

An opened real file is checked at4bytes, appended by a second handle, and shared bounded reading stops at5bytes. Existing counted-reader drop tests also pass after the helper rename. A valid symlink preserves bytes; directories reject; a FIFO through the public loader returns Failed within a test-side2second guard. All existing drop, viewer and image parsing tests pass.

The shared helper preserves the prior drop open flags and regular-file checks; drop extension/anchor gates remain at their previous call site. Candidate R20's unused synchronous constructor was not changed or removed. No full UI, Windows, allocator/RSS or aggregate-concurrency test was performed. The limit governs new source copying/consumption, not pre-existing Arc allocation or conversion buffers.
