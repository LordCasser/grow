## Tasks

- [x] Add a client-surface contract for structurally valid received message and reply rows, stable receipt identity, terminal `Received` state, and malformed-notice fallback.
- [x] Project valid receipts through the existing communication tool-row renderer and carry immutable receipt identity through scrollback deduplication/replay.
- [x] Preserve Minimal's print-once frontier across receipt replay and notice-to-row reconstruction.
- [x] Add projector tests for message, reply, duplicate/replay including cursor-tail and Minimal frontier merges, child ownership/replay eligibility, missing body, and invalid identity.
- [x] Run focused Pager tests and strict OpenSpec validation; record verification results.
- [x] Update the local coordination architecture note with the received-row behavior.
