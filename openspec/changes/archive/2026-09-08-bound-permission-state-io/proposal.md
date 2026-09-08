# Why
Permission actor startup awaits an unbounded read_to_string of permission state. A FIFO can block without a writer; oversized regular files allocate without limit.

# What Changes
Admit only ordinary opened files with at most 1 MiB serialized UTF-8 TOML. Bound actual read to limit+1 bytes, use Unix nonblocking open and same-handle metadata. Reject oversized snapshots before atomic publication.

# Impact
Rejected reads use default remembered state, preserve source and do not import shared grants. Existing missing-file fallback and malformed schema handling remain. No new dependencies.
