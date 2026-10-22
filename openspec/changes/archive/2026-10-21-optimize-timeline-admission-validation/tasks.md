## 1. Baseline

- [x] 1.1 Add an ignored repeatable cold Timeline replay benchmark for 128, 512, and 2048 valid response admissions.
- [x] 1.2 Verify benchmark fixtures fold to the same Surface and preserve representative admission errors; record pre-change timings.

## 2. Implementation

- [x] 2.1 Maintain completed-attempt and seen-admission indexes in accepted Timeline fold state.
- [x] 2.2 Replace clone-and-quarantine validation with a read-only quarantine count; add a narrowly scoped healthy-append summary only if the measured count scan warrants it.
- [x] 2.3 Add differential and boundary coverage for failure atomicity, request attempts, duplicate identities, malformed tool identity, and Surface replacement/rewind/compaction.

## 3. Verification

- [x] 3.1 Compare the post-change replay benchmark against the recorded baseline using the same command and profile.
- [x] 3.2 Run focused tests and strict OpenSpec validation; record exact results before closing the change.
