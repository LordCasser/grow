## Why

Four public convenience request methods infer `PermissionRequestSource` from optional session metadata. One path with a session ID but no subagent display type silently becomes Primary, which could apply the wrong permission scope. Shell production already passes an explicit typed context.

## What Changes

Remove the ambiguous convenience request methods from production builds. Keep strict test-only shorthand for existing manager tests, accepting only a wholly absent Primary identity or a complete Child identity; all production requests use `request_with_context` and explicit `PermissionRequestSource`.

## Capabilities

### Modified Capabilities

- `tool-authorization`: make request source explicit at the production authorization boundary.

## Impact

No production caller needs migration. Test helpers reject partial identity rather than silently changing scope; the public Rust API is narrowed without backward compatibility scaffolding.
