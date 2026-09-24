## Why

Pager's general ACP notification router can associate an unmatched session ID with the active root while that root is waiting for its assigned session ID. That startup convenience is unsafe for permission requests: an unrelated raw session ID can be presented as the active root's authorization request.

## What Changes

- Permission admission requires an exact registered root or child session identity.
- The startup race fallback remains available to ordinary session updates, where it preserves early display updates.
- Unmatched permission requests are cancelled without entering any permission queue or changing permission mode.

## Capabilities

### Modified Capabilities

- `client-surfaces`: Pager permission routing rejects requests without an exact session owner.

## Impact

Pager ACP permission admission and focused routing coverage. No protocol fields or permission-manager behavior change.
