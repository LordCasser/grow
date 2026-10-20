# Verification — 2026-10-20

## Root causes and scope

The release GCC diagnostics are not all cosmetic. In upstream sqlite-vec 0.1.10-alpha.4 (`04d28bd21773981e2d266bbf6aa4efbd011eb4f6`), a `size_t` initialized to -1 cannot satisfy `< 0`; an absent text metadata-IN entry therefore reaches an out-of-range address. Unsupported distance/type dispatch can leave a distance or blob length/offset undefined. The patch rejects those states through existing error cleanup, initializes absent PK length, expresses byte-size comparisons explicitly, replaces folding pragmas with comments and marks intentionally unused parameters. The build no longer suppresses unused-parameter warnings. Integer metadata-IN's existing signed sentinel is unchanged.

Grow's current production schema uses chunk_id/embedding and MATCH/k, not text metadata IN. The reproduction demonstrates a native backend defect, not an observed Grow user-session exploit. Vendoring now explicitly records maintained source changes rather than promising all upstream files remain byte-identical.

pprof 0.15 still requires nix ^0.26; upgrading Grow's separate nix cannot remove this copy. libc's musl `suseconds_t` deprecation concerns time64 ABI evolution. The patch normalizes microsecond **values** to i64; it preserves libc::timeval, raw constructor and FFI layout. No new deprecation suppression is added. See [libc ABI issue](https://github.com/rust-lang/libc/issues/1848).

`cargo install cross --locked --version 0.2.5` uses cross's independent old lock with yanked dunce 1.0.2, regardless of Grow's dunce 1.0.5. All three current cross platform entries use x86_64 ubuntu-24.04 runners. The workflow installs the unchanged [official cross 0.2.5 archive](https://github.com/cross-rs/cross/releases/tag/v0.2.5), both cross and cross-util, with independently measured SHA-256 `642375d1bcf3bd88272c32ba90e999f3d983050adf45e66bd2d3887e8e838bad`. This is a repository pin verified from the official download, not an upstream-published checksum. No unlocked resolution, fork, root lock edit or blanket warning filter.

## Executed checks

- Baseline GCC 16 `-O2 -Wall -Wextra -Werror` reproduced 13 diagnostics; patched code passes. AppleClang strict compilation and Linux GCC 14 compilation also pass.
- `tests/native_boundaries.c` includes the production vendor C source and links real SQLite. The old source under UBSan fails with `runtime error: applying non-zero offset 18446744073709551576 to null pointer` at old sqlite-vec.c:7003. The patched source passes missing/empty/nonmatching IN dispatch and invalid vector writer checks; invalid writer does not change the blob.
- Complete SQL harness passes float/int8 L1, L2 and cosine, bit search, text IN, explicit text PK, unsupported enum errors and a valid same-connection query after failure. These cover the unchanged Grow-shaped MATCH/k path too.
- Linux ASan/UBSan boundary-only run passes with leak detection enabled (`ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1`). Full SQL ASan/UBSan run passes address/undefined checks with leak detection disabled only for that manual run; see independent debt below. CI runs strict full SQL plus sanitizer boundary-only, without blanket warning/leak suppression.
- Actual vendored nix copied to a temporary standalone crate (not a new workspace member), temporary manifest pinned to root-lock libc 0.2.189: `cargo test --lib --no-default-features --features time,resource,signal,process,user,fs sys::time::test` passes **11 tests**, 0 ignored, on Rust 1.93.1. Includes negative-second normalization and comparison to the raw libc value.
- The same source `RUSTFLAGS='-D warnings' cargo check --no-default-features --features fs,signal` passes for native aarch64-apple-darwin, aarch64-unknown-linux-musl and aarch64-unknown-linux-ohos. These are compile checks, not musl/OHOS runtime tests. Root Cargo.lock is unchanged.
- Existing nix default/all-feature test builds expose unrelated test-only pin_utils unsafe/lifetime diagnostics under its deny(warnings); those are not silently reclassified as passing. Production fs/signal and the scoped 11 time tests above passed.
- Official archive checksum verified independently twice; extracted both executables. On an amd64 Rust 1.93.1 container, `cross --version` exits 0 and reports `cross 0.2.5` (then its ordinary host Cargo fallback). Offline attempts first failed because cross invokes rustup component/update setup even for --version; network-enabled verification succeeded. No old-lock source installation is performed.

Reproduction commands for the persistent C harness:

```sh
cc -O2 -Wall -Wextra -Werror -DSQLITE_CORE third_party/sqlite-vec/tests/native_boundaries.c -lsqlite3 -lm -o /tmp/grow-vec-boundaries
/tmp/grow-vec-boundaries
cc -O1 -g -Wall -Wextra -Werror -DSQLITE_CORE -fsanitize=address,undefined -fno-omit-frame-pointer third_party/sqlite-vec/tests/native_boundaries.c -lsqlite3 -lm -o /tmp/grow-vec-boundaries-asan
ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1 /tmp/grow-vec-boundaries-asan --boundary-only
```

## Separate debt and limits

Full SQL LeakSanitizer reports a pre-existing 40-byte text metadata shadow-name allocation from vec0_init (current sqlite-vec.c:5417). Normal vec0_free lacks the matching per-name release. Recorded in backlog for a separate lifecycle fix; not mixed into compiler/dispatch repair. The boundary sanitizer does not allocate that virtual table and keeps leak checking enabled. No assertion that full SQL is leak-free. macOS does not support the Linux leak detector; Linux container evidence is used for that check.

Harness uses system SQLite, not the entire Grow Cargo linkage. No full multi-platform Grow rebuild, hosted workflow or real OHOS-device run was triggered; future release gates still have to pass. Original v2.4.1 tag/assets and MCP are checked separately without mutation in the companion build-identity record.

## Completion checks

- Strict all validation before archive: 18 passed. Both changes archived with their delta merged; strict all validation after archive: **16 passed, 0 failed** (15 specs and unchanged active MCP change). Archive validation: **644 passed, 0 failed**.
- Final Python discovery rerun: **23 tests OK, 1 OHOS-only skip on host**; that case separately passed in the actual OHOS image. YAML, shell/Rust formatting and git diff checks pass.
- Public release API before/after equality covers release id/status/date and all 11 assets' id, name, size, digest and updated_at. Remote/local tag object `2b2a19332ee90fdf5378a5bccaaf4f6e7e5df92a` and peeled source `5fcbd12b0c10afef1f34695394d2ad81ff5b5310` remain identical. MCP branch `5b375b07c08ec113c06cb9fa426bd0353e114941`, pre-existing stash and root Cargo.lock are unchanged. See [immutable identity receipt](../2026-10-20-bind-release-build-identity/immutable-release-check.json).
- Repair verification performed no commit/push/tag/release or hosted build. Subsequent user-authorized main commit preparation rechecked strict validation (16 current items and 644 archived changes), diff hygiene and unchanged public release identities. No publication or MCP change is included.
