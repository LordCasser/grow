`cargo test --locked --offline -p client-support --lib clipboard --quiet`: 56 passed, 3 ignored, 0 failed. The ignored cases require real clipboard access. The focused suite includes the bounded `limit + 1` reader and oversized temporary-file rejection/cleanup.

`openspec validate isolate-macos-clipboard-image-reads --strict --no-interactive` and `openspec validate --all --strict --no-interactive`: passed (18/18).

Source inspection confirms both macOS explicit image entry points use their existing AppleScript/temp-file paths and that the in-process `dataForType:` reader and environment kill switch are absent. AppKit remains used for metadata-only `changeCount`/`types` probes. No live clipboard contents were accessed or modified.

The subprocess boundary isolates Grow's address space from pasteboard image materialization. It does not bound the `osascript`/AppKit helper's RSS, the temporary file's size before Grow reads it, or subsequent image decode memory. The simple image paste path adds the documented subprocess/file round-trip latency.
