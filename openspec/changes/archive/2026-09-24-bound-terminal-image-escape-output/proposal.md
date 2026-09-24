## Why

Kitty and iTerm2 image upload helpers currently create a full base64 `String` and then copy it into a second escape buffer assembled with formatting. Source image and converter limits therefore do not bound Grow's serialized terminal output or its intermediate copies. This change gives each Grow-built image escape buffer a strict 100,000,000-byte ceiling and rejects an upload that cannot fit.

## What Changes

- Bound each complete Kitty and iTerm2 image escape and the inline-media frame aggregate, including protocol headers and chunk framing, to 100,000,000 bytes.
- Encode image data directly into the bounded output buffer, avoiding a full-size intermediate base64 string and per-chunk formatted strings.
- Propagate oversize or allocation failure as no escape output so callers do not emit a partial image sequence.
- Count image-clear escapes in the frame budget and retain cleanup state when a clear does not fit.
- Clarify that this limits Grow-owned serialized buffers only; it does not control terminal-process decode or cache memory.

## Capabilities

### New Capabilities

### Modified Capabilities
- `client-surfaces`: image escape builders have a bounded output contract and fail closed when an upload exceeds it.

## Impact

`crates/codegen/pager-render/src/terminal/image.rs`, its focused tests, the inline-media caller in `crates/codegen/pager/src/app/agent_view/media.rs`, and the image resource-boundary entry in `openspec/backlog.md`. No new dependencies.
