## 1. Strict permission routing

- [x] 1.1 Add a permission-only exact session lookup while preserving the general startup fallback for ordinary notifications.
- [x] 1.2 Add deterministic coverage that an unmatched stranger request during root startup is cancelled and never queued.
- [x] 1.3 Keep existing exact root, child, unknown-session, and startup-update routing behavior.

## 2. Contract and validation

- [x] 2.1 Run focused Pager permission-routing tests and rustfmt.
- [x] 2.2 Run `openspec validate --all --strict --no-interactive` and record results in `verification.md`.
