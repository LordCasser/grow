# Change: Close platform-owned clipboard image allocation watch

## Why

The remaining clipboard image allocation risk occurs before Grow owns a bounded buffer: macOS `osascript`/AppKit can materialize or decode data inside the helper, and non-macOS platform backends can allocate RGBA before arboard returns it. The existing contracts already isolate or serialize these operations and cap data after it crosses into Grow. There is no demonstrated Grow-owned allocation left in this watch with a small additional fix.

## What Changes

- Remove only the “图片编码前的平台资源边界” entry from `openspec/backlog.md`.
- Record why the remaining OS-owned allocation risk does not presently justify a product change.

## Impact

Documentation-only. No runtime behavior or accepted contract changes; `skip_specs: true` applies. This closes follow-up tracking, not the OS-owned memory risk.
