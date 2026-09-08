## Why
Unoptimized session tests overflow the explicit 8 MiB test thread on both Linux CI and macOS. The loop future is 136272 bytes, but its Rust 1.93.1 unoptimized poll frame reserves 0x24b6f0 bytes. Nested admission and tracing poll frames exceed the test stack.

## What Changes
Align the compaction and stop/cancel test harnesses with the existing heavy session/image harnesses using a 32 MiB test stack. Preserve production's 8 MiB setting and every test assertion. This changes only test infrastructure, with no product behavior or delta. Production boxing experiments were ineffective and have been reverted.
