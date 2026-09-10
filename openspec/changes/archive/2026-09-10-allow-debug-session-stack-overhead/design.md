# Design

Select the existing session-thread stack constant with `cfg!(debug_assertions)`: 32 MiB for unoptimized development builds; the established 8 MiB for release builds. Rust's explicit `thread::Builder::stack_size` overrides RUST_MIN_STACK, so the process harness cannot enlarge these threads by setting the test-runner environment.

# Evidence and rejected experiment

The unchanged independent-process coordination harness reproduces SIGBUS/KERN_PROTECTION_FAILURE at the stack guard in `process_conversation_turn` before the mock model receives a prompt. The nested async poll frames are unoptimized. Heap-pinning the three call sites was tried but did not eliminate the debug temporary-frame overflow; that experiment was fully reverted. In response to the user's direction, retain production control flow and accommodate debug resource overhead directly.

# Verification

Rebuild the debug CLI and run the existing independent-process regression, without altering any scenario. Run relevant shell regression tests. Confirm `release-dist` inherits release and debug assertions remain disabled. Optimized cross-platform binaries are built by the established release workflow.
