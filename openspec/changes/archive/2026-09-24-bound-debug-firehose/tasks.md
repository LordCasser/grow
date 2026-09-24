## Implementation

- [x] Replace per-session sinks with one bounded file stream and explicit session attribution.
- [x] Bound producer record/queue memory and worker file bytes; coordinate retention across processes and heal path replacement.
- [x] Update developer and user documentation; retire obsolete debug contracts and backlog entry.

## Verification

- [x] Test concurrent sessions, overflow/loss, file cap, cross-process lock, replacement and bounded flush.
- [x] Run focused Rust tests, formatting, diff check and pre-archive strict OpenSpec checks.
