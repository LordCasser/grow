## 1. Validate dispatch

- [x] 1.1 Reject a selection before queue mutation or side effects unless its ID is present in the exact front request.
- [x] 1.2 Add a regression for a child request receiving a crafted global Always Approve selection while the root is active with a stale child marker; assert no response, pop, or mode change.
- [x] 1.3 Retain valid selection response and queue-transition behavior through focused tests.

## 2. Contract and validation

- [x] 2.1 Remove only the resolved selection-dispatch clause from the authorization provenance backlog entry; retain ownership and raw-session follow-ups.
- [x] 2.2 Run focused Pager tests, rustfmt, and `openspec validate --all --strict --no-interactive`; record results in `verification.md`.
