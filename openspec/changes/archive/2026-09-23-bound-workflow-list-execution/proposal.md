## Why

`grow/workflows/list` performs filesystem discovery and script reads directly in an asynchronous extension request. A slow or blocked filesystem can occupy a Tokio worker and stall unrelated requests without a deadline.

## What Changes

- Run workflow listing scans in a bounded blocking worker with a five-second deadline that includes waiting for worker capacity.
- Return timeout and worker failures as errors, never as an empty successful workflow list; keep an uninterruptible scan's slot until its worker exits.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workflow-execution`: Bound asynchronous workflow listing and define timeout/failure behavior.

## Impact

Changes the `grow/workflows/list` handler in `crates/codegen/shell/src/extensions/skills.rs`, uses the existing workflow registry scanner, and adds focused tests and developer documentation. No wire shape or dependency changes.
