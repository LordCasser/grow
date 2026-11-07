# Align SessionUpdate fixture with test-support feature gate

## Why

`crates/codegen/shell/tests/test_grow_session_update.rs` fails to compile as an integration target on default features: its `totalTokens` match (line 269) matches `SessionUpdate::ResponseReplayProjection`, but that variant only exists in `crates/codegen/shell/src/session/storage/mod.rs` under `#[cfg(any(test, feature = "test-support"))]` (storage/mod.rs:3005). Integration targets compile the library as a normal dependency without `cfg(test)`, so the arm references a variant that is absent there. This is a fixture-only defect from when the variant became storage-only; no production variant was removed.

## What Changes

Gate the existing match arm with `#[cfg(feature = "test-support")]`: the test compiles on default features (where the enum has only `Acp`/`Grow`, so the remaining arms stay exhaustive) and keeps the arm under `--features test-support`. The Acp/Grow `totalTokens` assertions are unchanged; no library code, feature defaults, wildcard arms, or new tests are introduced.