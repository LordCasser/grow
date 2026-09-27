## Why

Permission Sidebands intentionally overlap live foreground attempts. Provider usage is only known after an attempt returns, so already-admitted concurrent attempts can together pass a Goal token threshold. The current code closes admission after durably recorded usage reaches the threshold, but the contract does not state whether the threshold is a hard reservation limit.

## What Changes

Define token_budget as a settled-usage admission threshold, as requested: once confirmed cumulative usage reaches the limit, new provider admission closes; already-admitted work finishes and is fully charged. Keep independent permission Sidebands parallel. Add a multi-attempt regression.

## Capabilities

### Modified Capabilities

- `behavior-goal`: state concurrent budget admission and overshoot accounting.

## Impact

No runtime admission change; normative budget semantics and coverage become explicit.
