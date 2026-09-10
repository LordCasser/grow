# Verification

## Frozen real-session comparison

Used a read-only copied, complete-line prefix from session `01a081be-6168-7772-9e0c-dcc62a76b552`: 203,528,591 bytes, 66,069 Timeline events. No original session files were modified and no writer/resume path was invoked. No conversation payload is included in this record or repository.

Command: `GROW_REPLAY_BENCH_PATH=/tmp/grow-resume-timeline.jsonl cargo test -p chat-state --lib benchmark_frozen_timeline_replay -- --ignored --nocapture`, with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`.

Both paths ran in the same unoptimized test binary. Baseline uses the retained public transactional `accept`, equivalent to the previous `from_events` loop. Event decoding and the baseline input clone are outside fold timing.

| Phase | Time |
| --- | ---: |
| JSONL decoding | 22.394 s |
| Previous transactional fold | 88.316 s |
| Private bulk fold | 1.543 s |
| Fold speedup | 57.24× |

The comparison passed for Surface contents/IDs, lifecycle ownership, pending Control transitions and next event sequence. Baseline cumulative times were 2.05 / 8.03 / 18.18 / 32.22 / 50.53 / 72.67 seconds at 10k / 20k / 30k / 40k / 50k / 60k events, consistent with cumulative cloning overhead.

These are Timeline-fold measurements, not release-binary or end-to-end resume timings. File integrity/artifact verification, JSON parsing, actor setup and TUI updates replay remain outside the speedup claim. No provider calls or credentials were required.

## Regressions

- Initial chat-state suite: 466 passed, 0 failed, 1 ignored (manual benchmark).
- Large request history matches transactional acceptance; duplicate request IDs and invalid content reject in both paths. Failed live acceptance leaves prior state unchanged.
- Control replay comparisons cover pending transitions before terminal admission and final activation; existing suites cover image projections, recovery, inputs, ownership and notifications.
- Final chat-state suite: 466 passed, 0 failed, 1 ignored (14.31 s).
- Shell suite: 3,767 passed, 0 failed, 3 ignored (44.54 s), including storage corruption rejection, replay, resume, Goal/Workflow and image-recovery regressions.
- The final synthetic equivalence helper also checks prompt indices, notification receipts/pending queues, terminal tasks/monitors and subagent-result state.
- OpenSpec pre-archive strict validation: 18 passed, 0 failed.

- Post-archive strict validation: 17 passed; archive validation: 301 passed, no failures.
- `git diff --check` passed.
