# Design

Both `get_image` and `get_attachments` currently call `native_image_read` before their script fallback. Remove that short circuit so both entry points use their existing `osascript` transfer protocol and bounded `read_clipboard_image_from_class` path. The subprocess runner already owns a five-second deadline, process-group cleanup, bounded stdout/stderr, and private per-call temporary directories; the image payload itself is transferred via file, then Grow reads at most 50,000,001 bytes and rejects over-limit data.

Delete `native_image_read`, its paste-time-only UTI selection helper, native-read tests, and `GROW_CLIPBOARD_NO_NATIVE_READ` documentation. Preserve the shared pasteboard lock and runtime AppKit access used by metadata-only probes. Update the metadata worker contract to state that explicit image reads always use the subprocess path, including while native metadata is stalled.

The tradeoff is explicit: a simple paste now pays the `osascript` plus temporary-file round trip (historically about 0.5–0.9 seconds). The process boundary protects Grow's address space from `dataForType:` materialization; it does not constrain the helper process's RSS, AppKit allocation, disk usage before the bounded read, or platform decoding.
