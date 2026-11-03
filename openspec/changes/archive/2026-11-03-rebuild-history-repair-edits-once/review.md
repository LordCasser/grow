# Independent review

Result: no remaining production blocker found. This was a static review against the two source files in the snapshot named by `/tmp/grow-history-repair-baseline-path.txt`, followed by review of final differential tests, preserved probes and raw measurements. The reviewer did not run Cargo or modify production/tests.

## Algorithm and persistence semantics

- Quarantine computes all ranges and evidence from the original slice. Ranges remain ordered, nonempty and disjoint because their Reasoning/BackendToolCall prefixes and ToolResult suffixes cannot cross another Assistant. The retain_mut counter advances on every original element, including removed elements. Replacement at range.start and the half-open skip_until boundary correctly handle adjacent ranges and preserve original evidence order.
- Dangling repair preserves its original discovery pass, immediate-result boundary, call order and reason text. The merge cursor counts only original items consumed; inserted synthetic items do not shift later planned coordinates. Empty plans retain the existing Vec. Tail and successive insertion sites therefore have the same meaning as the previous reverse-splice application.
- Duplicate removal still uses a separate last-occurrence map for each Assistant's contiguous result run. Earlier result coordinates are unique, sorted once, and removed by one original-coordinate retain pass. It does not deduplicate across run boundaries. The removed count remains exact.
- Quarantine, normalization, dedup, displaced-result stripping and dangling synthesis keep their original pipeline order. Normalize/strip, report fields, Timeline repair proof rules and durable ACK paths were not changed. Empty plans do not rebuild conversation storage. In particular, report.changed()==false still means all item values are unchanged; the future C4 reuse of a healthy candidate can rely on that property.

## Oracle and coverage

The test-only old editors were checked against the before snapshot. Variable renaming and shortened loops retain the old operations: reverse splice, reverse-position remove and its same-run rescan. They do not delegate editing to the new algorithms. Quarantine shares only the unchanged range/evidence helpers. The sampling-types oracle shares the unchanged reason formatter; the mixed chat-state oracle independently reproduces its three reason strings.

Permanent tests compare complete serialized structures and counts. They exercise adjacent quarantine ranges, private reasoning exclusion, public backend evidence, a valid sibling whose ID makes a later malformed exchange duplicate, all dangling reasons, SkillLoad boundaries, three-result last-wins and repeated IDs in different runs. The mixed pipeline test checks all five report fields with nonzero effects, complete output equality, and an unchanged second pass. Existing repair, reason, malformed-identity and persisted old-repair replay tests remain in the owner-reported full suites. The new mixed fixture itself is textual and is not an added media-heavy or Unicode stress test.

## Measurement and space tradeoff

Both preserved probes clone input before starting the clock. They stop the clock before serializing/comparing the final output and before dropping the remaining repaired Vec. Drops/removals and buffer release performed by the repair itself remain inside timing, as does quarantine's necessary evidence JSON serialization. Old/new runs alternate per pair with two warmups and seven samples. Complete output JSON and return counts are checked in every pair. All 60 paired raw rows have correctly reported medians.

The measurement correctly reports sparse regressions: at N=8192/K=1 quarantine rises from about 63 to 160 microseconds and dedup from 40 to 139 microseconds. The larger-K curves show the benefit from removing repeated suffix movement; this does not establish a speedup for every workload. The N=128/K=64 duplicate fixture actually contains 192 items, and that exception is explicitly documented. These are debug synthetic editor-kernel measurements, not full repair/Timeline or production throughput measurements.

There is no allocation/peak-memory measurement in this change. Static space costs are real: quarantine prepares K replacement items and their complete evidence before editing; dangling merge temporarily owns the old backing array and a new N+S array; dedup accumulates all removal coordinates instead of only one run's coordinates. A one-edit dangling input with spare Vec capacity can therefore allocate more than the old splice. Quarantine avoids a separate N-item output array, but K can itself grow with N. The change trades these local costs for fewer repeated moves; it should not be described as zero-allocation or uniformly lower peak memory. No threshold or second algorithm path is required by the supplied evidence.

## Owner-executed validation

`verification.md` reports final post-probe-removal runs with `--locked`, isolated GROW_HOME and RUST_MIN_STACK=16777216: sampling-types 320 passed/0 failed, chat-state 606 passed/0 failed/3 ignored. Targeted differential tests and both explicit probes also passed. These commands were run by the implementation/measurement owner, not by this reviewer. Temporary probe functions and fixture helpers are absent from the final build sources; their source and raw samples remain in this change.
