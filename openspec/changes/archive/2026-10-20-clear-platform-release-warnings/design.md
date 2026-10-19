# Design

Retain the managed-config parent directory handle on every platform: rename the deliberately held resource rather than removing the Windows open or its failure path. The Unix sync still uses that same handle. Keep LiveCwdScan::Failed as the shared fail-closed state even on platforms with no enumerator; document a narrowly scoped dead-code allowance there, while compiling scan validation helpers only for Linux/macOS or their tests.

Clipboard reads obtain the optional original backend error from the match result rather than initializing and overwriting a mutable slot. Linux's guarded empty result remains None and still reaches its CLI fallback; all other empty/success answers return exactly as before. Import Command/Stdio only where the Linux fallback uses them.

Other fixes remove an unused Windows handle trait, keep DirBuilder mutation inside Unix mode configuration, and compile login-shell state only where it is read. No broad lint suppression, skipped tests, runtime changes, or MCP changes. The official Windows build treats Rust warnings as errors with RUSTFLAGS=-D warnings, so successful x86_64 and aarch64 release builds provide a persistent gate. This does not claim zero vendor C or cross bootstrap diagnostics on other platforms.

The old annotated tag is private/unreleased; first confirm cancellation and no public release, then replace it with an annotated tag on the corrected main commit. The official workflow must rebuild and revalidate all platforms and gates on that new immutable SHA. Previously successful artifacts cannot be reused as the final release assets.
