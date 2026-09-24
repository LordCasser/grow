# Verification

## Source and contract review

- `openspec/specs/client-surfaces/spec.md` records the macOS five-second AppleScript lifetime, 50,000,000-byte per-file write cap and 50,000,000-byte encoded-image read cap, and explicitly excludes helper/AppKit memory and later decoding.
- `crates/codegen/client-support/src/clipboard.rs` applies the file-size limit to the macOS child process. The non-macOS arboard image worker holds one permit while reading and encoding; its caller wait is two seconds, and the permit remains owned by a late worker. The RGBA pixel and PNG output checks run after `get_image()` returns. Linux helper capture also has an encoded-byte limit.
- `2026-09-24-isolate-macos-clipboard-image-reads` documents AppKit data materialization being moved out of Grow's address space and its remaining helper/AppKit RSS risk. `2026-09-24-bound-macos-clipboard-transfer-file` adds a per-file write cap without asserting an RSS bound.

No low-cost Grow-owned mechanism can cap helper/AppKit or platform-backend allocations before the data crosses the process/API boundary. A deadline bounds lifetime, not peak RSS; `RLIMIT_RSS` is not a reliable portable enforcement mechanism. Backend replacement or OS-level isolation would be disproportionate absent target-platform measurements and a concrete supported enforcement approach. The backlog item was removed with this residual risk explicitly retained in the decision record; no hard memory bound is claimed.

## Checks

- `openspec validate close-platform-image-resource-watch --type change --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 16/16.
- `git diff --check -- openspec/backlog.md openspec/changes/close-platform-image-resource-watch`: passed.

No product code was changed; Cargo was not run.
