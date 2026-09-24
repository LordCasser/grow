# Design

Use `diagnostics::instrumentation::timer` and the already-installed instrumentation layer. Keep each timer around one real production operation: replay `end_batch`, frame render closure, terminal cell diff and frame enqueue, and background `write_all`/flush. Record frame output bytes and aggregate counts, sums, and maxima in the ignored PTY test. Enable instrumentation through `GROW_INSTRUMENTATION=log` plus a per-fixture log path in the isolated sandbox.

These timers may overlap across the event loop and writer thread, and the log includes a few post-load key-echo frames; do not sum them into an exact wall-clock decomposition. The same-process Shell `session.load_session` timing can be compared to the PTY visible-history timestamp without cross-harness ambiguity. Keep the default path free of extra tracing events.
