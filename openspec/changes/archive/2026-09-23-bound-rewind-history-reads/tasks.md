# Tasks

- [x] Bound pinned rewind record and scan bytes; move full and metadata reads off the async runtime with safe cancellation ownership.
- [x] Validate nested metadata records and propagate picker errors through Shell/ACP/Pager.
- [x] Cover over-budget and damaged records, retry, cancellation, and UI-visible failure; update developer guidance.
- [x] Run targeted Rust checks, OpenSpec strict validation, archive and remove the resolved backlog entry.
