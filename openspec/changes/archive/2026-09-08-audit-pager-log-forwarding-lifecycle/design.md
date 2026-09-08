# Evidence
- pager/src/unified_log.rs buffers in a process-global Vec. push_entry drains at 16 once ACP_TX exists. send_entries then asks Handle::try_current; absence returns after ownership of the drained batch is lost. The public module promises logging from anywhere. Current inspected UI callers normally run within the runtime; this audit does not claim a live user report reproduced on a plain thread.
- flush drains before checking ACP_TX; flush_blocking does likewise. app/mod.rs calls flush_blocking on connection failure before event_loop initializes the sender. A pre-init flush therefore discards buffered entries rather than preserving them for later initialization.
- init ignores OnceLock::set failure and still starts another interval task; duplicate initialization creates duplicate periodic consumers while continuing to use the first sender. Actual full/headless entrypoints are separate normal startup paths, not proof that production calls init twice.
- Each send_entries spawns a detached acp_send task. flush_blocking drains only BUFFER and awaits only its own new batch. If BUFFER is empty it returns immediately even with earlier detached batches pending. Thus its documentation claiming delivery before shutdown exceeds its mechanism.
- acp-transport/src/channel.rs::acp_send enqueues AcpArgs on an unbounded MPSC and waits for a oneshot. ExtNotification is internally request/response-shaped too; message.rs invokes agent.ext_notification before responding. Merely enqueueing or dropping the response receiver would not prove completion of shell ingestion.
- shell/src/agent/mvp_agent/acp_agent.rs parses grow/log and calls diagnostics::unified_log::ingest_client_entries before returning. No filesystem writer changes are needed to address pager ownership.
- Before init, BUFFER has no count/byte limit. After init, 16-entry batches bound only the current Vec, not detached tasks, ACP queue, message bytes or shutdown wait duration.

# Boundaries
No real logs, global logger initialization or running app mutation. No claim of failed disk durability, observed memory exhaustion, or current plain-thread production caller. Hidden /debug is release-registered but only debug-listed; its scroll/log/FPS actions are real functionality. No additional deletion candidate follows from optional visibility.

# Next decisions
Separate ownership/delivery repair from overload policy. First change must retain a valid initialization runtime for arbitrary-thread dispatch, avoid draining before a sender exists, and make duplicate init idempotent. Shutdown acknowledgement and bounded producer/queue budgets require their own explicit designs; do not call flush_blocking fully durable after only runtime capture.
