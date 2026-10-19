# Verification

Validated on macOS arm64 with repository-pinned Rust 1.93.1 and isolated test homes.

- Pager library: 7,517 passed, 12 explicitly ignored. Shell library: 4,266 passed, 5 explicitly ignored.
- Real parent/child export covers multibyte paths/identity and unpacked transcript separation. The production HTTP regression passes real durable ledgers through capture, rendering, writes, synchronization, tar creation, response streaming and archive-content assertions.
- Publication collisions, write-failure cleanup, directory synchronization, and HTTP response completion/abandonment are covered. Unix directory synchronization remains strict; non-Unix skips only unsupported directory-handle synchronization, retaining file write/sync errors.
- The production HTTP fixture installs the repository TLS provider, disables proxies, bounds the request, and awaits server cancellation.
- Native macOS/Linux/Windows workflow selectors include these production export tests. Hosted Windows/Linux execution remains pending; this local result is not Windows execution.
- Formatting, diff whitespace and strict OpenSpec validation passed. See ../2026-10-19-enforce-release-regression-gates/verification.md for aggregate evidence.
