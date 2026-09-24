## 1. Bound pre-session command discovery

- [x] 1.1 Reuse the shared bounded worker for the full pre-session non-chat catalog path while keeping chat and live-session early returns.
- [x] 1.2 Add meaningful request-branch coverage for unchanged chat and live-session early-return behavior; the shared worker's deterministic timeout, failure, and permit-retention injection coverage remains in `extensions::skills::tests`.
- [x] 1.3 Document the execution boundary, update this backlog sub-item after verification, and validate/archive the change.
