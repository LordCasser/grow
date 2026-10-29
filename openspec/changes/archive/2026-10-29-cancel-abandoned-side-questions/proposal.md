# Cancel abandoned side questions

## Why

`SessionCommand::SideQuestion` starts independent local work, but the worker does not observe whether its oneshot response receiver still exists. A caller that disconnects can therefore leave image projection waits, provider requests, retries, and Sideband usage work running without a consumer.

## What Changes

- Cancel a side-question future when its response receiver closes, including while the future is running.
- Keep session Sideband cancellation and the existing Sideband drop cleanup as the owners of provider cancellation, usage settlement, and terminal repair.

## Capabilities

### Modified Capabilities

- `model-sampling`: define the response receiver as part of the side-question execution lifetime.

## Impact

The change is limited to the `SideQuestion` dispatch in `shell/src/session/actor/run_loop.rs` and focused lifecycle regression coverage.
