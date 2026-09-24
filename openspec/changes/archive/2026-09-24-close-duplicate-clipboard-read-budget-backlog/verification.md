# Verification

## Source audit

- `openspec/specs/client-surfaces/spec.md` defines the 5-second / 1 MiB-per-stream AppleScript process boundary and the 50,000,000-byte encoded-image boundary for both native reads and fallback files.
- `crates/codegen/client-support/src/clipboard.rs` checks `NSData.length` before Rust buffer allocation/copy, and the fallback reader consumes at most limit + 1 bytes. The reader is synchronous and has no independently owned cancellable worker.
- The same module's non-macOS `arboard_get_image` reads in the existing 2-second worker wait and converts the returned RGBA to PNG. Input shape is validated; the separate backlog item tracks the platform memory/pixel budget.
- `crates/codegen/pager-render/src/prompt_images.rs` caps each path image and the aggregate image bytes at 50,000,000 bytes. Its dimension probe calls `validate_image_bytes_unrestricted(..., false)`, which parses headers without a full pixel decode.
- The remaining macOS AppKit initialization and locked native-message execution deadline is already recorded by the separate “剪贴板原生元数据调用期限” backlog entry, whose scope explicitly includes held native messages.

No unique actionable clipboard read/decode budget remains in this broad entry. Adding a local-file timeout without an owner for the synchronous read would leave the operation running after timeout; designing such an owner is separate work.

## Checks

- `git diff --check -- openspec/backlog.md openspec/changes/close-duplicate-clipboard-read-budget-backlog`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 16/16 before archive.
- `openspec validate close-duplicate-clipboard-read-budget-backlog --type change --strict --no-interactive`: passed.
- Archived with `openspec archive close-duplicate-clipboard-read-budget-backlog --skip-specs --yes`; this is documentation-only and merges no specification delta.
- Post-archive `openspec validate --all --strict --no-interactive`: passed, 15/15.
- Post-archive `openspec validate --archived --strict --no-interactive`: passed, 453/453.

No product code, specifications, or tests were changed or run.
