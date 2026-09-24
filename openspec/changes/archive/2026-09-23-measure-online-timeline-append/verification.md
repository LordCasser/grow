# Verification: measure-online-timeline-append

## Scope and method

The live path is transactional: `Timeline::prepare` calls `validate`, which clones `LifecycleFold`; `Timeline::accept` validates again and makes a second clone before applying the event. Actor durable writes prepare before persistence and accept only after persistence succeeds. The focused harness measures the Timeline prepare+accept portion and excludes actor persistence and scheduling.

An ignored unit benchmark built histories through public recording methods, then measured seven successful assistant-message append samples per case. It varied prior assistant-message event/Surface history (0, 1,000 or 10,000) independently from retained completed request identities (0, 100 or 1,000). It pre-grew event and Surface vector capacity before sampling. A test-only allocator wrapper counted requested `alloc` and `realloc` bytes during each sample; the benchmark ran alone with one test thread. Figures are unoptimized test-profile medians, not release or end-to-end timings.

Command:

```sh
CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test -p chat-state --lib benchmark_online_timeline_append_cost -- --ignored --nocapture --test-threads=1
```

## Results

| Prior message events | Retained completed requests | Base events / Surface items | Median append | Median requested allocation |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 0 | 34 / 32 | 2,000 ns | 800 B |
| 1,000 | 0 | 1,034 / 1,032 | 1,959 ns | 800 B |
| 10,000 | 0 | 10,034 / 10,032 | 1,917 ns | 800 B |
| 0 | 100 | 234 / 32 | 22,875 ns | 14,116 B |
| 1,000 | 100 | 1,234 / 1,032 | 21,167 ns | 14,116 B |
| 10,000 | 100 | 10,234 / 10,032 | 21,208 ns | 14,116 B |
| 0 | 1,000 | 2,034 / 32 | 189,416 ns | 131,604 B |
| 1,000 | 1,000 | 3,034 / 1,032 | 189,292 ns | 131,604 B |
| 10,000 | 1,000 | 12,034 / 10,032 | 189,917 ns | 131,604 B |

At the tested sizes, append cost was insensitive to prior message/event history and scaled with retained lifecycle identities. This narrows the backlog item to lifecycle-fold cardinality; it does not establish a product latency failure or justify a behavior change without representative production lifecycle distributions or an existing latency budget.

The benchmark also submitted an empty append at the next sequence. Acceptance rejected it, and a full Timeline debug snapshot before and after compared equal. The test passed: 1 passed, 0 failed; the measured sweep took 0.71 s after compilation.

## Checks

- `openspec validate measure-online-timeline-append --strict --no-interactive`: passed before archive.
- `git diff --check` on tracked audit code and the backlog edit: passed; OpenSpec strict validation covers the change records.
- Cargo verification was limited to the focused ignored benchmark; the harness reports its rejection-atomicity assertion in the same run.
- `openspec archive measure-online-timeline-append --skip-specs --yes`: archived without specification changes.
- Post-archive `openspec validate --all --strict --no-interactive`: passed, 20/20.
- `openspec validate --archived --no-interactive`: passed, 420/420.
