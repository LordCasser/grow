## Why

The image normalization permit currently covers separate blocking transcode and compute closures, while `normalize_one` decodes the base64 input and re-encodes transcoded or compressed output outside those closures. Those staging buffers can overlap work admitted for the next image, so the existing single-worker contract does not cover the full per-image normalization pipeline.

## What Changes

- Run base64 decode, optional endpoint transcode and PNG encoding, normalization compute, and output base64 encoding under one process-wide worker permit for each `normalize_one` call.
- Keep the permit inside that single blocking closure so cancellation of the async waiter cannot admit another image while any stage continues.
- Preserve existing image acceptance, conversion, compression, and failure behavior.

## Capabilities

### New Capabilities

### Modified Capabilities
- `client-surfaces`: one worker permit covers the complete `normalize_one` pipeline, including its encoded staging buffers.

## Impact

`crates/codegen/shell/src/session/image_normalize.rs`, its focused tests, the `client-surfaces` contract, and the image resource-boundary entry in `openspec/backlog.md`. No new dependencies.
