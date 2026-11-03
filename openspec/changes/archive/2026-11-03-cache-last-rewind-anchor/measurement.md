# Prompt cache key microbenchmark

Command:

```sh
GROW_HOME=/tmp/grow-home-c3 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib actor::request_builder::tests::prompt_cache_key_reverse_scan_vs_index_microbenchmark -- --ignored --exact --nocapture
```

Raw output was captured at `/tmp/grow-backlog-c3-key-benchmark.txt`. The complete temporary Rust source, including fixture generation, exact old scan, warmups, sampling and median calculation, is preserved in [probe.rs](probe.rs). The benchmark compared the exact prior reverse-scan key implementation with the indexed `prompt_cache_key` implementation in the same test process, on the same immutable Timeline fixture for each case. Fixture creation and key equivalence checks were outside the timed regions. Each case used two warmups followed by seven raw timed samples; each sample includes full key composition and hashing, with no Timeline clone. Both paths produced identical key strings for every case.

| Events | Rewind position | Reverse scan median (ns) | Indexed median (ns) |
|---:|---|---:|---:|
| 128 | none | 3625 | 2583 |
| 128 | old | 3625 | 2667 |
| 128 | latest | 2667 | 2583 |
| 2048 | none | 16916 | 2625 |
| 2048 | old | 16750 | 2625 |
| 2048 | latest | 2958 | 2916 |
| 8192 | none | 66167 | 2625 |
| 8192 | old | 65709 | 2916 |
| 8192 | latest | 2667 | 2625 |

Raw samples (nanoseconds), reverse scan then indexed:

```text
128 none:    [3709, 3708, 3625, 3667, 3584, 3584, 3625] / [2625, 2625, 2583, 2583, 2583, 2583, 2625]
128 old:     [3625, 3584, 3583, 3708, 3542, 3667, 3708] / [2791, 2792, 2625, 2667, 2750, 2625, 2584]
128 latest:  [2666, 2708, 2667, 2667, 2708, 2625, 2583] / [2625, 2583, 2583, 2583, 2583, 2625, 2542]
2048 none:   [16958, 16958, 16833, 16916, 16917, 16875, 16834] / [2625, 2708, 2583, 2583, 2625, 2583, 2667]
2048 old:    [17208, 16625, 16875, 16875, 16584, 16667, 16750] / [2667, 2666, 2625, 2583, 2542, 2625, 2625]
2048 latest: [2959, 3000, 2958, 2875, 2916, 2958, 2917] / [2916, 2917, 2916, 2917, 3000, 2916, 2875]
8192 none:   [59292, 64875, 66667, 66500, 66167, 66167, 60625] / [2666, 2625, 2625, 2625, 2583, 2625, 2583]
8192 old:    [66333, 66166, 60500, 59375, 59625, 65709, 66417] / [2959, 2959, 2916, 2875, 2875, 2917, 2916]
8192 latest: [2708, 2708, 2583, 2625, 2708, 2667, 2625] / [2584, 2667, 2625, 2625, 2583, 2625, 2584]
```

This is a local microbenchmark, not an end-to-end request build measurement. The largest difference is expected when no Rewind exists or the latest Rewind is old; the latest-Rewind case remains near key hashing cost in both paths.
