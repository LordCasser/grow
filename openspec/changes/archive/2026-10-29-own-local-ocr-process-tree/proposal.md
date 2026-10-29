# Own the local OCR process tree

## Why

Local OCR currently enables Tokio `kill_on_drop` and waits for its direct child, but timeout or cancellation can leave helper descendants running. Those descendants can also keep the captured output pipes open after the caller no longer owns the OCR operation.

## What Changes

- Run each local OCR process in an owned process group/job using the existing `tty-utils` process-group lifecycle.
- Timeout, cancellation, errors, and normal OCR completion terminate any remaining process-tree descendants while preserving direct-child wait/reap.
- Add deterministic descendant cleanup coverage for cancellation, timeout, and successful direct-child exit.

## Capabilities

### Modified Capabilities

- `model-sampling`: local OCR cancellation owns and terminates its helper process tree.

## Impact

The change is limited to local OCR process ownership and its focused tests in `shell/src/session/image_describe.rs`. It reuses the existing `tty-utils` process-group helper and preserves the current OCR deadline and output bounds.
