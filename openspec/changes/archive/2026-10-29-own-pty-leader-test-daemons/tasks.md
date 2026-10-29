## Tasks

- [x] Make `LeaderCluster` own the concrete persistent leader through `TestProcess`.
- [x] Verify normal drop and panic cleanup ownership using existing and real-daemon lifecycle coverage.
- [x] Validate and record verification.

- [x] 真实 Grow daemon 在普通 Drop 和模拟断言 unwind 后均被 wait/reap，运行专用 ignored 回归。
