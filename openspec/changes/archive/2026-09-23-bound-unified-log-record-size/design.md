## Boundary

`emit` builds Shell entries, while `ingest_client_entries` accepts Pager batches and stamps each entry with the client source. Both paths converge on `LogEntry` serialization in `diagnostics::unified_log`; no producer-specific byte limit can protect the persisted JSONL file. Today both paths call `serde_json::to_vec`, which allocates the full encoded line before writing.

## Decision

Set a 64 KiB maximum per complete JSONL line, counting the trailing LF. Stream JSON serialization into a writer that checks the next chunk against the remaining byte budget before extending its `Vec`. This gives the encoded buffer a fixed logical size ceiling without first constructing an unbounded temporary string or vector. A rejected serialization is discarded before any file write.

For an oversized entry, serialize a compact replacement entry through the same bounded writer. Keep its typed source, level and producer PID; generate a fresh timestamp; use a fixed version marker; omit the session and all original message/context values; and include a fixed `record_omitted` reason and byte limit in context. This makes the data loss explicit while bounding attacker-controlled client fields out of the fallback. The replacement is well below the production budget and ends in one LF.

Apply the same encoder to Shell emits and every Pager entry in a batch. Serialization completes per record before the existing batch append, so no partially encoded record can reach the file. Existing append locking and best-effort I/O handling remain unchanged.

## Validation

Exercise a record with a multi-megabyte field and verify the encoded result is at most 64 KiB, has exactly one terminal LF, deserializes as a `LogEntry`, and contains the omission reason with no original oversized data. Exercise a normal record to verify it remains unchanged. Run the focused diagnostics unit tests, formatting, diff checks, and strict OpenSpec validation before archive.
