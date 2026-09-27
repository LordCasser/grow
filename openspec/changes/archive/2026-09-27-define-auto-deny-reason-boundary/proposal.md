## Why

Two manager tests expect free-form LLM `reason` in `PolicyDeny`, while the current implementation returns a fixed safety instruction. Passing classifier prose into the calling Agent's tool result would turn lower-trust model output into new instructions. The contract does not yet state which reason is authoritative, leaving tests and implementation in conflict.

## What Changes

Define the classifier's reason as internal diagnostic data. Denied tool results and permission audit projections use bounded harness-owned reasons. Update the two stale tests to assert this boundary and retain model source/verdict evidence.

## Capabilities

### Modified Capabilities

- `tool-authorization`: define the reason exposure boundary for Auto denial.

## Impact

No runtime decision or persistence-format change. Tests and the normative authorization contract become consistent with the existing safe behavior.
