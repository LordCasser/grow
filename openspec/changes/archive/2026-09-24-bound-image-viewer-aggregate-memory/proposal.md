# Change: Bound aggregate image viewer memory

## Why

Each image viewer has a 50 MB encoded-source allowance and conversion output is bounded, but separate Agent views can load concurrently and retain both the source and display bytes. Closing or reopening a viewer drops its visible state, yet a late background result still owns its buffers until delivery. The number of retained and in-flight buffers has no process-wide bound.

## What Changes

- Admit viewer loading through one process-wide byte budget before copying source bytes or converting; account for worst-case conversion workspace and final encoded buffers.
- Transfer the retained reservation with a loaded result to its viewer and release it when the viewer/result is dropped, including stale results, close, and reopen.
- Fail a load through the existing failed-preview path when the process budget cannot admit it, without blocking input or leaving a loading viewer indefinitely.
- Measure controlled aggregate reservation and representative conversion memory in focused tests.

## Capabilities

### Modified Capabilities

- `client-surfaces`: aggregate viewer admission and release.

## Impact

The existing background completion owner check remains authoritative. No input image, model request, or terminal graphics protocol changes.
