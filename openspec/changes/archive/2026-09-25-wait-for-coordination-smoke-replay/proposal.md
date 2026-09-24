## Why

The v2.2.0 macOS ARM64 release asset built and signed, but its coordination smoke test failed while reading the recovered inquiry notifications immediately after the second `session/load` reply. The fixture reads ACP notifications on another thread; that thread can process an already-sent notice after the reply wait completes. The failure dump contains both terminal notices from the first load, while the immediate second-load assertion observed none.

## What Changes

- Wait a bounded time for each expected replayed terminal notice to arrive in the fixture reader, then retain the exact-one and outcome assertions.

This only repairs smoke-test synchronization and does not change Grow behavior or the existing coordination contract, so no spec delta is needed.
