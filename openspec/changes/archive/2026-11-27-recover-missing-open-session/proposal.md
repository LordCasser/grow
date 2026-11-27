## Why

An open Pager can outlive its resident Shell actor on the same transport. Foreground prompts and model changes then repeatedly fail at handle lookup while the original journal and view remain available. A second defect masks typed session-info failures in the UI.

## What Changes

Reconnect the exact open primary session to its durable history after a confirmed missing-handle response to a current user request. Preserve the rejected input for explicit retry and show the durable lookup outcome in Session Info.

## Impact

Pager request completion, in-place resync, and Session Info decoding. A typed missing-residency code in ACP error data and Shell supervisor diagnostics for unexpected resident exits. No change to journal facts, prompt admission or authorization.
