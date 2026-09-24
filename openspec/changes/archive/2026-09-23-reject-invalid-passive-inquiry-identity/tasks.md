## 1. Identity admission

- [x] 1.1 Read the `client-surfaces` and `local-coordination` contracts; inspect inbound audit validation, all `upsert_coordination_row` callers, scrollback lifecycle handling, and existing regressions.
- [x] 1.2 Reject absent/empty identity safely and preserve malformed incoming notices as finite notices.
- [x] 1.3 Add targeted regressions for missing identity, empty identity, and empty notice correlation ID; retain lifecycle regression coverage.
- [x] 1.4 Run targeted Pager validation, narrow only proven backlog claims, and complete OpenSpec validation/archive.
