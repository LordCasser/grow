# Tasks

- [x] Update the visual auxiliary and local OCR fallback contract, including the English operation-row lifecycle and original-language recognition output.
- [x] Replace dynamic source-text prompting with a fixed English prompt; use that prompt identity for cache and completed-Sideband recovery.
- [x] Add a typed image-recovery lifecycle and render it as one stable, mutable Pager/Minimal row instead of NOTICE messages.
- [x] Settle terminal, partial, failed, cancelled, and replayed operation states without leaving an unresolved spinner or interrupting a live reconnect.
- [x] Add client-surfaces scenarios and focused UI lifecycle/replay coverage.
- [x] Add provider-request-boundary coverage proving history, tool/Skill context and source message text are absent while images and the fixed prompt remain.
- [x] Review cancellation, cache identity, durable recovery, and stale terminal ordering.
- [x] Validate the complete OpenSpec change and record the parent-run Rust verification results.
