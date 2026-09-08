# Design

Change the shared pinned-file JSONL reader to return the first deserialization error instead of logging and skipping it. Include the diagnostic file label and one-based physical line number. Build the vector locally so a valid prefix is not exposed on failure. The existing FileStateTracker load error path retains the source and does not merge any prefix.

Pinned metadata scans use the same reader: malformed records for that schema now trigger their existing in-memory-only fallback, without consuming the source. This does not make metadata schema validation equivalent to full FileSnapshot validation; metadata intentionally counts/skips snapshot contents. Legacy path readers are cfg(test)-only and retain their fixture behavior; do not delete them in this fix. Update the old test that conflated path-reader and active pinned-reader leniency.

Test a valid prefix and suffix around invalid syntax or invalid row fields, live in-memory state, failing mutations, metadata fallback, exact physical line diagnostics and a repaired-source retry. Preserve blank-only/empty inputs and valid final JSON without a newline. Existing resource budgets and blocking reads remain separate.
