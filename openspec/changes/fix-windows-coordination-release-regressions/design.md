# Design

Peer publication uses `SetFileInformationByHandle(FileRenameInfoEx)` on the validated staging file with REPLACE_IF_EXISTS and POSIX_SEMANTICS. An open reader retains the old file object while new opens resolve to the replacement. Keep owner-only DACL creation, no-reparse verification, long-path encoding and bounded sharing-error retries; do not delete the destination before publication.

For independent session loads, open each contained path component with the existing shared-read operations. Keep cached writer capabilities authoritative when the adapter already owns one. An independent observation handle does not enter that cache; explicit writer admission still opens a pinned capability and acquires the same exclusive lease. Workflow restoration reads reuse the opened session capability rather than reopening through the writer path. Existing Summary projection reconciliation remains unchanged.

The actor fixture uses `std::env::temp_dir()` for its mock cwd. Existing tests are retained: manifest replacement with a held reader, live sideband observation followed by exactly-one writer recovery, and all coordination actor cases.

# Evidence

Foreground startup failure reports the actual completed RPC instead of waiting only on the model event; bounded recent fixture notices and permission requests accompany stderr diagnostics. This preserves every regression assertion and the existing timeout for a genuinely pending request.

Windows job 102846210811 in run 34469589238 reports Error 5 on replacement, Error 32 on observer load, and NotAbsolute for `/tmp`. Existing `ContainedDirectory::open_shared_read` already documents and handles retained DELETE-capable publication handles; the ordinary load route was not using it.

Microsoft documents the rename flag union for [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info) and old-handle/new-name semantics for [FileRenameInformationEx](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/4217551b-d2c0-42cb-9dc1-69a716cf6d0c). No backward-compatibility fallback is added for filesystems that reject the required atomic replacement semantics; failures preserve the old manifest.

The PTY test harness import now mirrors its existing Unix-only export and call site. Coordination CI retains dependency caches on failure using the same rust-cache setting as core/storage validation; this does not change test selection.

The disk-usage symlink fixture calls its existing Windows directory/file helpers, with Unix aliases to the existing generic helper. This restores cross-platform compilation without removing or weakening any metadata-only, no-follow or independent-billing assertion.

Failure diagnostics are emitted after fixture clients close, before their temporary directory is removed: each process exit code and the last 16 KiB of each fixture stderr. The explicit `process_only` dispatch option skips already-passed, unchanged library suites while rebuilding the CLI and executing every real-process scenario. Normal dispatch and pull-request behavior remain complete. Validation records must identify these targeted reruns and retain the full library-run provenance.

The diagnostic Windows run proves a debug main-thread stack overflow (`0xC00000FD`, stderr names `main`). `RUST_MIN_STACK` applies to spawned Rust threads, not the main thread ([Rust documentation](https://doc.rust-lang.org/std/thread/index.html#stack-size)). Only coordination CI links its Windows debug CLI with `/STACK:33554432`, reserving stack through the supported [MSVC linker option](https://learn.microsoft.com/en-us/cpp/build/reference/stack-stack-allocations?view=msvc-170). No production call chain or release linker setting changes. Native release smoke steps additionally run the existing complete isolated process regression against the finalized distribution executable before packaging, attestation or publication; this independently checks optimized binaries with their real stack settings.
