# Healthy request copy probe

Temporary ignored probe ran on 2026-11-03 with `--locked`, isolated
`GROW_HOME=/tmp/grow-c4-home`, `RUST_MIN_STACK=16777216`, and one test thread.
The exact fixture, clone/repair candidates, equality check, warmups, repetition
count, and timing loop are retained in [probe.rs](probe.rs). Raw output was
written to `/tmp/grow-request-integrity-probe-raw.txt` and copied below.

The comparison uses the old healthy path (clone Surface, run the original
repair function, drop that copy, clone Surface again for the request) and the
new candidate (clone once, run the same repair function, reuse that vector).
Each fixture was serialized and compared before timing. Fixture creation,
serialization/equality checks, and sorting samples were outside timed regions.
Each result is the average of 16 operations; there were 2 warmups and 7 raw
samples per case. This measures only the healthy clone/repair kernel. It does
not measure actor message passing, persistence, memory lookup, Goal projection,
image projection, or end-to-end provider request time. Image URLs are short
`Arc<str>` payloads; cloning their bytes is not implied by these timings.

This is evidence for the healthy clone pass only. When a new MemoryContext is
being committed, the owned request vector stays live across its ACK while
`commit_timeline_event` may temporarily copy the Surface for context-pressure
refresh. That can increase peak live vectors relative to the old ordering.
No C4 heap or RSS measurement was run, so these results do not claim lower
peak memory for requests that add a new memory reminder.

| Fixture | Items | Old median (ns/op) | Reused median (ns/op) |
|---|---:|---:|---:|
| text | 128 | 27,338 | 14,765 |
| text | 512 | 117,968 | 63,132 |
| text | 2,048 | 460,906 | 246,111 |
| text | 8,192 | 1,841,726 | 976,921 |
| tool calls/results | 128 | 182,820 | 165,567 |
| tool calls/results | 512 | 710,617 | 642,382 |
| tool calls/results | 2,048 | 2,807,349 | 2,519,309 |
| tool calls/results | 8,192 | 11,120,789 | 10,087,867 |
| image Arc | 128 | 33,018 | 17,851 |
| image Arc | 512 | 129,684 | 70,312 |
| image Arc | 2,048 | 533,703 | 285,226 |
| image Arc | 8,192 | 2,121,127 | 1,125,770 |

## Raw samples

Each array is in ns/op, in execution order; the median is the middle sorted
sample.

```text
kind=text items=128 reps=16 old_ns=[27403, 27338, 31906, 27315, 27315, 27356, 27299] old_median_ns=27338 new_ns=[14781, 16888, 14765, 14734, 14791, 14757, 14737] new_median_ns=14765
kind=text items=512 reps=16 old_ns=[112351, 115994, 116528, 117968, 118981, 119203, 119099] old_median_ns=117968 new_ns=[61742, 62130, 62125, 63632, 64083, 63132, 63463] new_median_ns=63132
kind=text items=2048 reps=16 old_ns=[434609, 458552, 457458, 460906, 461484, 463968, 464151] old_median_ns=460906 new_ns=[244382, 242257, 244458, 246111, 247460, 248224, 247351] new_median_ns=246111
kind=text items=8192 reps=16 old_ns=[1801609, 1815703, 1849161, 1843078, 1797838, 1841726, 1853559] old_median_ns=1841726 new_ns=[972395, 971281, 1003278, 976888, 976921, 992330, 997221] new_median_ns=976921
kind=tool items=128 reps=16 old_ns=[182820, 182453, 176731, 181580, 183401, 183606, 184416] old_median_ns=182820 new_ns=[166343, 160260, 163367, 165463, 167138, 165567, 167992] new_median_ns=165567
kind=tool items=512 reps=16 old_ns=[695156, 707440, 715489, 712450, 710617, 714187, 700635] old_median_ns=710617 new_ns=[633460, 641179, 642382, 642674, 646143, 644536, 632093] new_median_ns=642382
kind=tool items=2048 reps=16 old_ns=[2831466, 2813421, 2787906, 2757403, 2807349, 2770783, 2808690] old_median_ns=2807349 new_ns=[2554533, 2529164, 2527317, 2515278, 2515997, 2519309, 2509768] new_median_ns=2519309
kind=tool items=8192 reps=16 old_ns=[11097869, 11108927, 11120789, 11234239, 11071658, 11144718, 11158578] old_median_ns=11120789 new_ns=[10087867, 10082833, 10072471, 10088950, 10189593, 10053940, 10101190] new_median_ns=10087867
kind=image items=128 reps=16 old_ns=[33179, 33101, 32971, 33351, 33018, 32989, 32872] old_median_ns=33018 new_ns=[17343, 18153, 17065, 17934, 17851, 16914, 18278] new_median_ns=17851
kind=image items=512 reps=16 old_ns=[129684, 134380, 129466, 128960, 127966, 130213, 131671] old_median_ns=129684 new_ns=[70312, 70687, 68507, 68106, 70312, 70179, 71250] new_median_ns=70312
kind=image items=2048 reps=16 old_ns=[532812, 551338, 533528, 533812, 532843, 533703, 535078] old_median_ns=533703 new_ns=[285648, 284364, 283856, 285226, 283898, 286377, 287729] new_median_ns=285226
kind=image items=8192 reps=16 old_ns=[2071041, 2128599, 2104026, 2135724, 2121127, 2093924, 2146296] old_median_ns=2121127 new_ns=[1127221, 1137273, 1124450, 1097924, 1086677, 1125770, 1142221] new_median_ns=1125770
```
