# Change: Bound non-macOS clipboard image work

## Why

Non-macOS clipboard reads can acquire RGBA bytes through arboard and encode PNG without a pixel or output allowance. A timed-out arboard read may continue on an abandoned worker while another read starts. Linux clipboard helper output is collected without a byte allowance. Each can multiply image memory before the later normalization budget applies.

## What Changes

- Admit at most one in-process arboard image read/encode worker at a time, keeping the permit in the worker after caller timeout.
- Reject RGBA above 16,000,000 pixels before PNG encoding and cap encoded PNG at 50,000,000 bytes.
- Capture Linux CLI image output up to 50,000,001 bytes, rejecting overflow without retaining unbounded output.

## Capabilities

### Modified Capabilities

- `client-surfaces`: clipboard image read and encode boundaries.

## Impact

Text clipboard reads, macOS native `NSData` acquisition, and downstream normalization remain separate. This closes the Grow-controlled non-macOS image work, not the OS-owned pre-return allocation or aggregate queued payload budget.
