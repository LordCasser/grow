# Paired replay and storage measurements

Stage note: these root-directory measurements use CLI3 and the build12/13 performance executable, before the recovery delivery-order and restored-Hook fixes. They are retained as intermediate evidence; final acceptance requires the separately saved `final-paired/` rerun against the final build.

The paired before/after runs show the expected storage consolidation and faster history delivery on the representative modern fixtures. Each RPC row below summarizes three paired runs; values are medians with the observed min–max range in milliseconds. `RPC` is the complete `session/load` request, `first` and `last` are first and final history delivery, and `gap` is the largest gap between history updates.

| Fixture | RPC, before → after | First, before → after | Last, before → after | Max gap, before → after |
|---|---:|---:|---:|---:|
| modern, 128 turns | 92.701 [88.865, 99.993] → 96.534 [87.794, 105.171] | 37.389 [37.360, 39.590] → 21.268 [20.103, 22.833] | 40.915 [40.707, 43.241] → 24.626 [23.055, 26.179] | 1.158 [1.105, 1.169] → 1.269 [1.147, 1.357] |
| modern, 512 turns | 207.874 [206.769, 209.219] → 148.516 [147.687, 155.671] | 117.214 [116.634, 118.322] → 63.625 [63.274, 64.893] | 154.032 [152.733, 154.842] → 96.425 [95.455, 98.940] | 13.600 [12.831, 13.715] → 10.013 [9.002, 10.582] |
| modern, 2,048 turns | 695.378 [676.954, 718.940] → 399.315 [398.480, 415.460] | 452.918 [443.449, 469.671] → 230.170 [229.857, 235.736] | 641.375 [619.504, 659.170] → 378.128 [377.085, 397.142] | 54.098 [51.467, 55.779] → 20.111 [11.478, 26.044] |
| modern repair, 512 turns | 384.165 [370.415, 427.972] → 149.204 [149.044, 155.899] | 293.878 [283.186, 336.668] → 63.393 [62.888, 64.769] | 329.489 [318.269, 371.779] → 95.415 [94.104, 98.957] | 13.431 [13.307, 13.783] → 9.395 [8.561, 11.311] |
| modern short, 512 turns | 110.570 [109.544, 110.966] → 108.148 [105.911, 110.007] | 41.861 [41.237, 42.212] → 29.896 [29.583, 29.951] | 56.963 [56.141, 57.035] → 42.771 [42.327, 43.011] | 5.467 [5.424, 5.652] → 1.926 [1.850, 1.953] |

The largest fixture reduced `session/load` from 695.378 ms to 399.315 ms (42.6%); first history arrived in 230.170 ms instead of 452.918 ms (49.2% faster). At 128 turns, total load was effectively flat and its three-run range overlaps, while first and last history delivery improved. The 512-turn and short fixtures also show lower first/last delivery and smaller maximum gaps.

The directory now holds one fewer file and about half the bytes in every profile:

| Fixture | Directory bytes, before → after | Files, before → after |
|---|---:|---:|
| modern, 128 turns | 9,263,748 → 4,679,257 (−49.5%) | 5 → 4 |
| modern, 512 turns | 37,042,333 → 18,706,802 (−49.5%) | 5 → 4 |
| modern, 2,048 turns | 148,178,363 → 74,849,404 (−49.5%) | 5 → 4 |
| modern repair, 512 turns | 35,980,484 → 18,706,802 (−48.0%) | 5 → 4 |
| modern short, 512 turns | 7,682,205 → 4,026,738 (−47.6%) | 5 → 4 |

The separate updates file goes from 4,820,762 / 19,284,122 / 77,140,456 bytes for the 128 / 512 / 2,048-turn modern cases to zero. The Timeline grows to contain those typed records: at 2,048 turns it changes from 71,032,018 to 74,843,515 bytes. Total directory size still falls by 73,328,959 bytes. This is consolidation, not removal of the history payload.

## Durable response commit

Each commit benchmark has 21 response rounds; the table reports the median of three run medians and their range in milliseconds. The barrier count is per response and covers only Timeline and updates ledger file/directory barriers.

| Payload | Stage | Total | Candidate anchor append | Timeline admission append | Projection build | Projection commit | Barriers / response |
|---:|---|---:|---:|---:|---:|---:|---:|
| 4 KiB | before | 21.551 [21.527, 26.372] | 4.269 [4.249, 4.644] | 8.488 [8.457, 9.372] | 0.029 [0.028, 0.036] | 8.522 [8.520, 9.256] | 6 |
| 4 KiB | after | 13.066 [13.033, 13.290] | 4.485 [4.462, 4.503] | 8.575 [8.528, 8.891] | 0 | 0 | 4 |
| 32 KiB | before | 22.502 [22.466, 31.870] | 4.497 [4.434, 12.096] | 8.942 [8.541, 12.547] | 0.100 [0.098, 0.103] | 9.034 [8.985, 11.313] | 6 |
| 32 KiB | after | 13.080 [12.988, 13.124] | 4.430 [4.098, 4.494] | 8.570 [8.534, 8.990] | 0 | 0 | 4 |

The two fewer barriers come from removing the secondary projection write. The Timeline admission remains the main durable operation, with a median near 8.5–8.9 ms; candidate-anchor append remains near 4.3–4.5 ms. Total median commit time falls about 39% for 4 KiB and 42% for 32 KiB. The measurements count only these two ledgers and do not include summary, artifact, or other system calls.

## Interactive PTY replay

For each modern PTY workload, three runs were collected. Values are median [min, max] milliseconds. `History visible` is when the final history sentinel becomes visible, not first content; `Ready` is the subsequent SessionLoaded/replay-end boundary.

| Turns | Metric | Before | After |
|---:|---|---:|---:|
| 128 | History visible | 192.6 [169.6, 1,345.2] | 164.3 [162.0, 166.1] |
| 128 | Ready | 285.7 [218.2, 1,419.7] | 196.0 [192.2, 207.3] |
| 512 | History visible | 486.5 [433.5, 578.8] | 419.6 [416.1, 454.8] |
| 512 | Ready | 546.7 [514.9, 658.1] | 449.7 [445.6, 484.9] |

The before 128-turn range includes a single 1.35-second run and remains part of the data; it is not excluded from the reported three-run range. Other before/after runs are in the saved PTY measurements.

The separate slow-writer case is one run per stage and regressed. Before, first history was 310.3 ms, history completed at 530.7 ms, with 3 advances and a 112.0 ms maximum content gap. After, first history was 317.9 ms, completion was 573.3 ms, still with 3 advances, and the maximum content gap widened to 150.3 ms. This single-run result must remain visible alongside the faster ordinary PTY cases; it is not enough data to characterize the slow-writer distribution.

## Memory readings

The paired 2,048-turn measurements report `process_peak_rss_bytes` median 836,714,496 bytes before (range 672,202,752–846,315,520) and 691,486,720 bytes after (range 685,047,808–859,865,088). The ranges overlap substantially. A separate coarse `ps` observer sampled the full process from fixture completion through exit, including agent startup/cleanup and observer overhead. Its peak RSS medians moved in the opposite direction: 695.3 MiB before (677.5–713.0 MiB) and 772.1 MiB after (725.2–818.9 MiB); its first samples were 429.3 MiB and 501.0 MiB respectively. These scopes and sampling methods disagree on direction, so this evidence does not support a claim that memory decreased. Treat memory change as unresolved.

## Remaining work in the representative trace

In the 2,048-turn after trace, `session.load.timeline_read_decode` takes 204.4 ms inside the 212.6 ms `session.load_light` interval. History projection then takes 7.8 ms, and `session.load_session_replay` spans 71.3 ms. First history is delivered at 230.170 ms. The overall `session.load_session` interval is 398.4 ms; actor spawn and registration later span about 82.0 ms. These intervals are nested and overlap: do not add them to estimate total load time.

The same trace before the change has 87.5 ms of Timeline decoding, 70.6 ms decoding the separate updates file, and 146.9 ms planning the response projection (142.2 ms in its nested canonical projection interval). The after trace has removed the separate updates decode and response-projection planning stages. Unified journal decoding is now the largest early stage because the Timeline contains both Domain and presentation records, but the combined decode/projection work is still lower than those separate before stages. Later replay and actor initialization remain visible costs after first history.

The `modern-repair-512` case is not an equal-work repair comparison. Its before fixture sets `missing_every=16`, so its updates file is smaller than the ordinary 512-turn case (18,222,273 vs. 19,284,122 bytes). The after fixture has no separate updates file and its unified Timeline/history size matches the complete ordinary 512-turn fixture. Its apparent reduction therefore combines removal of the old missing-projection repair path with a different amount of stored presentation history; do not treat that row as a like-for-like repair speedup.

Evidence: `paired-summary.json`, `paired-before-measurements.json`, `paired-after-measurements.json`, `paired-before-timelines.json`, `paired-after-timelines.json`, `paired-commit-measurements.json`, `pty-measurements.json`, `rss-diagnostic.json`, and the corresponding `paired-*-*.txt` trace captures in this change directory.
