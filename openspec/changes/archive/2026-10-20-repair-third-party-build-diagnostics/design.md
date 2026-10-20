# Design

## Evidence and boundaries

Atlas's local scope identifies vec0_metadata_filter_text at sqlite-vec.c:6752; release GCC logs and a native GCC -O2 -Wall -Wextra -Werror compile reproduce signedness, missing sentinel, unused parameter and uninitialized-dispatch diagnostics. metadataInIdx is size_t and initialized to -1, so <0 never detects absence. Use array length as the sentinel and compare equality before dereferencing. Error must pass through the existing rowid/statement cleanup. Cover empty and nonmatching entries with real SQLite blobs, plus a normal SQL text-IN query and Grow-shaped MATCH/k query.

Keep primary key pointer/length absence coherent with a zero length. Every float/int8 distance switch and outer element switch must fail via existing cleanup on an unknown value; the blob writer must return SQLITE_ERROR before calling SQLite on an unsupported element. Do not seed arbitrary zero distance or byte counts. Byte lengths from SQLite are nonnegative, chunk sizes are validated; make the unsigned size comparison explicit without changing valid arithmetic. Folding pragmas become comments and unused parameter markers use the existing UNUSED_PARAMETER macro, not compiler lint flags. Preserve SIMD/IVF/DiskANN feature defaults and ordinary search semantics.

nix 0.26 is still required by pprof 0.15; upgrading the unrelated workspace nix cannot remove it. Returning i64 from the two microsecond value helpers removes references to the deprecated ABI alias without guessing a replacement C layout. libc::timeval, raw constructor parameters and FFI conversion remain unchanged; convert the raw-field comparison in the resource test to i64. Negative-second normalization and narrow-platform conversions need regression coverage. No new allow(deprecated).

cross's official release has no arm64 Linux archive, but the current three cross catalog entries all run on ubuntu-24.04 x86_64. Install both cross and cross-util from the official 0.2.5 Linux archive with a pinned checksum, and prove its --version. The checksum is independently measured from the official download, not an upstream-provided checksum. No cross fork, registry cache mutation, unlocked install or dependency graph re-resolution is introduced.

## Validation

A standalone C harness includes the actual vendored implementation and uses SQLite APIs to exercise error boundaries; run with GCC strict optimized diagnostics and sanitizers where supported. Also run supported vector SQL queries. Test nix microseconds against its raw libc field and negative values, with native and musl compilation. Verify cross official archive checksum and executable identity. Preserve the v2.4.1 tag object/source and published asset identities with before/after read-only checks.
