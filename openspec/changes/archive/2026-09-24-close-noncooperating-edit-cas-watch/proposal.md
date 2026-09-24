# Change: Close the noncooperating editor CAS watch

## Why

The local `search_replace` adapter now compares exact source bytes under an advisory file lock across Grow processes, including parent aliases, and rejects a stale source before writing. The only remaining subclaim asks for an atomic content CAS against arbitrary external writers that do not participate in that lock. The supported generic file APIs do not provide such a predicate; keeping an open implementation item would imply a guarantee Grow cannot supply through its current filesystem boundary.

## What Changes

- Remove the resolved edit-conflict section from `openspec/backlog.md`.
- Keep the explicit noncooperating-writer limitation in the archived conditional-commit design, main tool contract and developer guide. This is a scope decision, not a claim that arbitrary concurrent external writes cannot race.

## Impact

Documentation-only. No runtime behavior or accepted contract changes; `skip_specs: true` applies.
