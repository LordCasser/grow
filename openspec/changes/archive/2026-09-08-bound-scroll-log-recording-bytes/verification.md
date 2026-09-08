# Verification

## Implementation
new initializes remaining_bytes to64*1024*1024. write_line checks UTF8 byte length plus newline before lazy open, subtracts accepted bytes, and stops on exact exhaustion or an oversized next line. stop explicitly flushes the accepted buffer before Disabled. is_active and existing runtime toggles therefore expose stopped state and construct a fresh capture on restart.

## Coverage
Small allowance tests use a multibyte JSON string to verify newline/UTF8 accounting, exact exhaustion, rejection of a later line, flush with flush_now=false, and no growth after disable. An oversized first line leaves an existing target unmodified; a separate new recorder has the default allowance and records without changing the old file. Existing scroll_log tests cover lazy paths, special files, stream recordings and runtime state.

## Consumer evidence
pager-pty-harness/scroll_matrix/log.rs parses complete JSON lines and group_streams permits one trailing unfinalized stream. No synthetic finalize is appended; a long harness waiting for an expected finalize can time out if recording reaches its cap.

## Limits
No64MiB allocation/disk fixture, installedCLI/Windows run or live long-duration recording. This limits bytes accepted by each recorder, not combined disk use across old captures or writes by external processes. No async IO or slow-filesystem timeout. IO failure during flush can still truncate already accepted data under existing best-effort policy.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib scroll_log --quiet:11 passed,0 failed,0 ignored;0.03s; process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
