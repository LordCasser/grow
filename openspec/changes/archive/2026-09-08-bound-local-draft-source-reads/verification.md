# Verification

## Scope
LocalDraftStore::load now opens with Unix O_NONBLOCK, checks regular-file metadata on that same File and compares length as u64. It passes the same handle to load_from_reader, which reads at most256KiB+1 and checks actual length before parsing. Nonregular sources return InvalidData without quarantine. Oversized ordinary files retain existing quarantine semantics.

## Tests
- Valid record padded to exactly256KiB loads unchanged. Open its File, observe metadata, append64 whitespace bytes through another handle, then invoke the production bounded parsing helper on the opened File. It returnsNone, consumes precisely256KiB+1, and moves the source to quarantine. Separately test static one-byte oversize.
- Unix regular symlink restores the record. FIFO without a writer and a directory returnInvalidData, remain in place, and create no quarantine. Worker completion has a2-second guard.
- Existing local_drafts module tests cover record/key roundtrips, corruption quarantine, retry deadlines, rekey and real AppView draft ownership/recovery.

## Limits
No Windows run or installedCLI exercise. The growth test deliberately enters the production helper after manual metadata observation rather than racing load nondeterministically. Slow filesystems still have no wall-clock read timeout. Quarantine path replacement races, retained quarantine storage and runtime write policy after errors are outside this read fix.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib local_drafts --quiet:11 passed,0 failed,0 ignored;0.12s; process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
