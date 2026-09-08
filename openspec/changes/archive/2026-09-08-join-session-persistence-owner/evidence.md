# Source audit (2026-09-08)

- session/persistence.rs new/load/fork constructors use tokio::task::spawn for SessionPersistence, retaining an Arc<dyn StorageAdapter> in that future. The adapter owns writer_leases. PersistenceHandle currently carries tx/noop/directory, no task completion.
- SessionPersistence::run waits for rx closure, flushes the pending buffer, and only then returns. FlushAndAck responds inside the receive loop; the task and storage remain alive afterward.
- session/actor/spawn.rs creates a separate std::thread and local Tokio runtime. Its completion waits on session_done_rx. SessionThread owns only the std::thread::JoinHandle.
- agent/mvp_agent/session_lifecycle.rs close_session_explicit sends Shutdown, removes the resident handle, then invokes drain_old_session_thread. agent_ops.rs drain polls SessionThread::is_finished and joins that OS thread; it has no observation of the parent-runtime persistence task.
- An earlier uncommitted fixture in restore-session-effort-before-actor-spawn failed same-process immediate reload with active-writer errors on first or second iteration. A three-process fixture isolated the separate effort bug. This change still needs a deterministic delayed-owner regression; the source establishes the missing join boundary, not an exhaustive account of every retained sender.

No Rust changes, builds or real-history writes were performed during this audit. The current model-effort fix remains separately archived. Disk was previously cleaned; do not start a build until the regression fixture is ready.
