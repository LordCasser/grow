# Design

Use `SessionMemory::is_enabled()` as the runtime admission state. Check it in the pre-compaction hook before snapshot capture and again after the await that captures the snapshot. `run_memory_flush` checks before acquiring its flush lock, so callers that already hold a frozen snapshot cannot continue after the session is disabled.

The final provider admission check belongs after the durable Sideband attempt acknowledgement and immediately before `run_provider`. If the state is off, close that attempt with the existing `Cancelled` Sideband outcome and skip HTTP. After an accepted provider response, check current memory state before deduplication and persistence. The write branch already obtains storage immediately before its synchronous write, so no extra same-state check is needed there.

Tests use the sampler-backed actor and controllable image projection persistence fixture. They invoke the existing `/memory off` built-in action to change runtime state, then exercise pre-compaction and flush execution with a finite loopback inference server. They assert provider requests and persisted memory files at the relevant boundaries.
