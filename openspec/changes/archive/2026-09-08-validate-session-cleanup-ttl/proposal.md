## Why
Positive TOML TTL integers are truncated to u32, potentially becoming zero; large valid u32 day counts can exceed the representable cutoff.

## What Changes
Use checked integer conversion for configured TTL and checked date arithmetic before any scan/deletion. Missing configuration retains the documented 30-day default; malformed/unrepresentable configured retention must fail closed rather than become a shorter policy. Inspect config-layer error behavior before choosing the exact parser boundary. Tests must include 2^32, u32::MAX, nonpositive values, default, ordinary TTL and cutoff overflow without invoking real-home cleanup.
