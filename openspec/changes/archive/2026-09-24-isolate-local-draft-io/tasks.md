# Tasks

- [x] Capture a controlled slow-I/O reproduction and define the worker handoff.
- [x] Move cwd resolution and LocalDraftStore operations to a serialized worker, keeping snapshot traffic bounded.
- [x] Apply recovery only to current bindings and live draft state; preserve ordered invalidation/rekey/retry semantics.
- [x] Await the latest eligible checkpoint on normal quit.
- [x] Run focused Pager tests, relevant PTY checks, strict OpenSpec validation, update docs/backlog, and archive.
