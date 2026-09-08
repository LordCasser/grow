# Verification

## Scope and rationale
capture_agent already rejects simultaneous composer/staged_prompt because no recovery ordering exists. validate_record now enforces the same invariant for loaded and written records. write validates before has_payload/remove, so invalid empty structures cannot delete a previous draft. Valid empty records still remove as before.

## Tests
- Store writes of two populated or two empty prompt fields returnInvalidData and preserve a prior valid record. A single staged prompt roundtrips, and a valid empty record removes it.
- A real test AppView with empty composer/session key receives a manually written conflicting JSON record with deferred Plan. sync_local_drafts quarantines the exact original bytes; composer and queue remain empty and deferred behavior remainsNone.
- Existing local_drafts tests cover valid composer and Behavior restoration, prompt RPC ownership, key isolation/rekey, byte allowance, special-file rejection and write retry.

## Limits
No claim that normal capture previously emitted this invalid structure: it already rejected it. This closes the persisted-record validation gap. No Windows or installedCLI run; no real user drafts modified.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib local_drafts --quiet:13 passed,0 failed,0 ignored;0.14s; process exited0. Existing macOS compact-unwind warning only. git diff --check passed.
