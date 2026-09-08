# Verification

## Implementation
open_writer preserves parent creation and lazy opening. OpenOptions write/create uses Unix O_NONBLOCK; metadata on the opened File must be regular before same-handle set_len(0) and BufWriter construction. Failure flows through existing write_line open-error branch to Sink::Disabled, which scroll_log_active reports as inactive.

## Coverage
- Existing file remains unchanged until first record, then contains only the new JSONL line.
- Unix FIFO without a reader and with a nonblocking reader: first write_line returns, recorder is inactive, subsequent write stays disabled; target remains. Directory also disables. Worker completion has a2-second guard.
- Normal symlink remains a symlink and its ordinary file target gets the new JSONL contents.
- Existing scroll_log filtered tests exercise diagnostic recorder and mouse/runtime toggle behavior.

## Limits
No installedCLI or Windows run. FIFO tests call actual recorder.write_line but not real terminal mouse events. No global write timeout, log growth cap, asynchronous IO or special-device interface support. Existing explicit ordinary-file truncation is preserved deliberately.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib scroll_log --quiet:9 passed,0 failed,0 ignored;0.03s; process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
