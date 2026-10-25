## Why

After a cold resume, the TUI can show 1.2M / 1M until a new message is sent, after which it returns to about 391K / 1M. ChatState restores context pressure from the entire raw Surface, including historical reasoning omitted by cold portable replay. The first real request then applies the correct request projection.

## What Changes

- Initialize restored context pressure from the existing portable history projection used after native continuation resets.
- Keep the raw Surface estimate as the signed-delta baseline, distinct from the projected input estimate.
- Preserve actual oversize context, provider anchors, subsequent request envelopes, and durable history.

## Capabilities

- `model-sampling`: cold context-pressure initialization.
