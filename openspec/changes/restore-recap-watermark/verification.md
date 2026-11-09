# Verification

- Inspected v2.6.2 and main (8fbc5218): restored primary API idle time and zero-initialized recap watermark permit a new automatic recap after historical replay.
- Inspected the supplied image: two different summaries cover the same completed work. This supports regeneration rather than duplicate delivery, but the original session journal was not available to prove its precise event ordering.
- Added four bootstrap/serialization regression tests, required explicit coverage in existing generated-recap test observation, and added a loopback-provider actor test covering generated persistence messages, HistoryFact serialization, restored automatic gating and repeatable manual recap. This is not a complete disk-backed session/load process restart.
- git diff --check passes.
- Rust compilation, rustfmt, Clippy and tests have NOT run: this environment has no cargo/rustc/rustfmt.
- OpenSpec strict validation has NOT run: openspec is unavailable.
- No commit or remote push performed. CI has not run for this patch.
- Not a complete migration: legacy facts without coverage and suppressed automatic results retain cold-load retry behavior. No inferred link to a Sideband result is treated as a committed display watermark.
