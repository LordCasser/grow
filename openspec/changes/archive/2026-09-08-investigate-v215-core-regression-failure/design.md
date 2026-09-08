# Scope and constraints
Core run 34213624575 at 03eb765a failed: three builtin extraction tests failed before shell test process aborted from stack overflow. Preserve failure evidence and isolate each cause; do not mask by disabling tests or blindly increasing stack sizes. Release is paused until all newly requested work and verification complete.

Detailed implementation decisions and delta scenarios must be completed before corresponding code changes.

## Initial evidence (not yet reproduced)
Builtin extraction calls `sync_managed_parent`, which converts a capability directory handle into std::fs::File and calls sync_all. cap-primitives 4.0.2 opens Linux directory capabilities with O_PATH; such handles cannot be fsync'ed. This is a candidate Linux-only cause, requiring a targeted reproducer before changing code. The 8 MiB session/thread test stacks are explicit, so RUST_MIN_STACK does not enlarge them. Isolate the unnamed overflowing thread rather than assuming the environment setting applies.

The concurrent-extractor test passed while single-extractor cases failed. This is consistent with sync failure after rename: each subsequent extractor skips the already-matching file and advances to the next file/marker. Therefore existence after repeated calls does not prove the transaction sync succeeded. A regression should call extract_builtin_files_transaction directly and require Ok on the first complete extraction, then verify managed files and marker; do not merely call the public best-effort wrapper.

This change currently records diagnosis only and changes no behavior contract. Any resulting implementation must add the relevant delta before editing production behavior.
