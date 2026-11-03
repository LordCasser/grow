# Independent review

Scope: only the C3 `last_rewind_seq` projection, its request-key consumer, tests and measurement. Existing image-offset and recovery changes were excluded. This review read source and captured logs; it did not run Cargo or modify production code.

Result: no actionable production defect found.

- `Timeline.events` is private and its only production mutation is the append at the end of `apply_validated_event`. The derived anchor updates there under the same `Messages`/`Rewind` condition as the previous reverse scan.
- Default starts with no anchor; derived Clone copies it; `from_events` reconstructs it through the shared replay apply path. No legal event-vector truncation, replacement or alternate append path was found.
- `prepare` is read-only and failed `accept` never applies. Actor durable commit waits for persistence before calling `accept`; a candidate Timeline may acquire an anchor without publishing it to the actor. Unconfirmed or failed persistence cannot advance the actor's anchor.
- The decimal `EventSeq::get()` representation, `root`, hash component order, separators and output formatting remain unchanged. Permanent differential coverage compares complete keys at 128/2048/8192 events with no, old and latest rewinds. The extended Timeline test also covers two rewinds, clone isolation, prepare, failed accept and cold replay. Existing durable-rewind coverage exercises the persistence ordering.

The preserved probe was subsequently reviewed: fixture creation and equality checks are outside timing; each sample runs one complete key build with `black_box`, without a normalization divisor or Timeline clone. It uses two warmups and seven samples, with the middle sorted sample reported as median. Its old path matches the prior key implementation. Raw log values match `measurement.md` and show the expected event-count dependence only when the reverse scan must traverse old or absent rewinds.

This is a debug, single-process, fixed-order microbenchmark using synthetic Recovery filler. Nanosecond values include clock overhead and are not a release or end-to-end throughput claim. The temporary test was removed from the build; its source remains in `probe.rs`. The rerun instructions were corrected to paste only the final test function, because the permanent differential test already retains the four identically named helpers. No rerun was performed by the reviewer.
