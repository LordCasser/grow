use parking_lot::Mutex;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use agent_client_protocol as acp;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, simplex};
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio::time::Duration;
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tracing::{debug, info, warn};

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    LineBufferedRead,
};

use crate::agent::config::Config as AgentConfig;
use crate::agent::init::{bootstrap, exit_on_config_error};
use crate::agent::mvp_agent::MvpAgent;
use crate::util::grow_home;
use dirs;

const MAX_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Configuration for periodic auto-update checking in leader mode.
///
/// When the leader is running for a long time, it periodically calls `check_fn`
/// to check for updates. The `check_fn` is responsible for both detecting
/// whether a newer version is available **and** downloading/installing it.
/// It returns `true` only when the new binary is on disk and the leader
/// should shut down so the next `connect_or_spawn` picks up the updated binary.
///
/// If the download fails, `check_fn` should return `false` so the leader
/// stays alive and retries on the next interval.
pub struct LeaderAutoUpdateConfig {
    /// Interval between update checks (default: 1 hour).
    pub check_interval: Duration,
    /// Async function that checks for, downloads, and installs an update.
    /// Returns `true` if the update was installed successfully and the leader
    /// should shut down. Returns `false` to stay alive (no update, or download
    /// failed).
    pub check_fn:
        Box<dyn Fn() -> Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync>,
}

/// Timeout for a single check_fn call. The check_fn may include both a
/// version check and a binary download, so this must be generous enough to
/// cover large downloads on slow connections. Kept in sync with the artifact
/// download request timeout (20 minutes) so the leader does not abandon a
/// transfer that is still within the HTTP client's budget. If the call takes
/// longer than this, we abandon the attempt and retry on the next interval.
/// The select! with the cancellation token ensures the loop remains
/// responsive to shutdown signals even while waiting.
const AUTO_UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// How long the auto-update shutdown waits for session actors to flush
/// before the leader exits. Aliases the shared
/// [`crate::agent::activity::SESSION_FLUSH_GRACE`] so this path and the
/// in-process agent's `/exit` / headless-quit flush cannot drift apart.
const AUTO_UPDATE_FLUSH_GRACE: Duration = crate::agent::activity::SESSION_FLUSH_GRACE;

/// Consecutive busy deferrals after which an installed update proceeds
/// anyway (with the graceful flush). Bounds how long a permanently-"busy"
/// signal — an orphaned parked interaction, a wedged turn — can pin the
/// leader to an old binary: ~24h at the default 1h check interval. Mirrors
/// the bounded-grace semantics of the `RelaunchForUpdate` drain.
const MAX_AUTO_UPDATE_BUSY_DEFERRALS: u32 = 24;

/// Bounded wait for the leader flock when it is held but no socket is bound yet
/// (a spawner mid-handoff, an old-flow client holding the flock across its ~10s
/// spawn window, or a same-version sibling briefly holding it). Exceeds that
/// old-flow window so a legitimately-spawning peer wins the race.
const LEADER_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(15);

/// Run the auto-update checker loop.
///
/// Periodically calls `check_fn` to check for, download, and install updates.
/// If `check_fn` returns `true` (update installed) and the agent is idle,
/// flushes every session actor ([`AgentActivity::flush_all_sessions`]) and
/// then cancels the provided token to trigger a graceful leader shutdown.
/// Connected clients will receive a `ShuttingDown` → `Shutdown` sequence and
/// can seamlessly reconnect to a new leader with the updated binary (via
/// `connect_or_spawn` → `resolve_exe_for_spawn`).
///
/// Idle means BOTH `agent_busy` is false (no IPC client request in flight)
/// AND `activity.is_busy()` is false (no running turn, parked interaction,
/// or live subagent). The second signal covers work that outlives an individual
/// IPC request and therefore is not represented by `agent_busy`.
///
/// If `check_fn` returns `true` but the agent is busy, the shutdown is
/// deferred until the next interval when the agent may be idle — bounded by
/// [`MAX_AUTO_UPDATE_BUSY_DEFERRALS`], after which the update proceeds
/// anyway (still flushing first) so a permanently-busy signal (orphaned
/// parked interaction, wedged turn) cannot pin the leader to an old binary
/// forever.
///
/// The `check_fn` call is wrapped in a `select!` with the cancellation token
/// and a timeout so that a stalled download cannot block the loop from
/// responding to shutdown signals.
///
/// This is extracted as a standalone function so it can be unit-tested
/// independently from the full leader infrastructure.
pub(crate) async fn run_auto_update_checker(
    config: LeaderAutoUpdateConfig,
    agent_busy: Arc<AtomicBool>,
    activity: crate::agent::activity::AgentActivity,
    cancel: tokio_util::sync::CancellationToken,
    shutdown_tx: tokio::sync::watch::Sender<crate::leader::ShutdownReason>,
) {
    let mut interval = tokio::time::interval(config.check_interval);
    // Skip the first tick (fires immediately)
    interval.tick().await;
    let mut busy_deferrals: u32 = 0;

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = cancel.cancelled() => break,
        }

        info!("Leader auto-update: running update check");

        // Run check_fn inside a select! with cancellation and a timeout so a
        // stalled network call cannot block the loop from responding to shutdown.
        // The check_fn may include a binary download, so the timeout is generous.
        let update_installed = tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            result = tokio::time::timeout(AUTO_UPDATE_CHECK_TIMEOUT, (config.check_fn)()) => {
                match result {
                    Ok(installed) => installed,
                    Err(_elapsed) => {
                        warn!("Leader auto-update: check/download timed out, will retry next interval");
                        continue;
                    }
                }
            }
        };

        if update_installed {
            let busy = agent_busy.load(Ordering::Relaxed) || activity.is_busy();
            if busy && busy_deferrals < MAX_AUTO_UPDATE_BUSY_DEFERRALS {
                busy_deferrals += 1;
                info!(
                    busy_deferrals,
                    "Leader auto-update: update installed but agent is busy, deferring shutdown"
                );
                continue;
            }
            if busy {
                warn!(
                    busy_deferrals,
                    "Leader auto-update: deferral limit reached while busy; shutting down anyway"
                );
            } else {
                info!("Leader auto-update: update installed and agent is idle, shutting down");
            }
            // Flush session actors BEFORE cancelling — cancellation drops
            // the LocalSet, which aborts actors mid-instruction.
            activity.flush_all_sessions(AUTO_UPDATE_FLUSH_GRACE).await;
            // Signal the shutdown reason BEFORE cancelling so the IPC server reads
            // AutoUpdate when it processes the cancellation.
            let _ = shutdown_tx.send(crate::leader::ShutdownReason::AutoUpdate);
            cancel.cancel();
            break;
        } else {
            info!("Leader auto-update: no update installed");
        }
    }
}

/// Spawn the agent inside a LocalSet and return a handle to the I/O future.
fn spawn_agent_local(
    agent_config: AgentConfig,
    memory_config: Option<crate::config::MemoryConfig>,
    outgoing: impl futures::AsyncWrite + Unpin + 'static,
    incoming: impl futures::AsyncRead + Unpin + 'static,
) -> impl std::future::Future<Output = Result<(), acp::Error>> {
    let (gw_tx, gw_rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(gw_tx);
    let mut agent = MvpAgent::new(gateway, &agent_config).unwrap_or_else(exit_on_config_error);
    if let Some(mc) = memory_config {
        agent.set_memory_config(mc);
    }
    let incoming = LineBufferedRead::spawn_local(incoming);
    let (conn, handle_io) = acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
        tokio::task::spawn_local(fut);
    });
    tokio::task::spawn_local(GatewayReceiver::new(gw_rx, conn).run());
    handle_io
}

/// Build a newline-terminated JSON-RPC request line for an internal
/// `grow/...` extension method, for injection into the agent's inbound ACP
/// stream by the leader's own watcher tasks (config hot-reload, skills).
///
/// The wire method is written **`_`-prefixed** (`_grow/internal/...`):
/// `agent-client-protocol`'s inbound decoder routes a non-built-in method to
/// `ext_method` only when it carries the `_` extension prefix and rejects
/// bare custom methods with `-32601 method_not_found`. These injections were
/// historically sent un-prefixed, so every watcher-driven hot-reload
/// (models, skills, MCP servers) was silently rejected at decode — the
/// watcher-side "change detected" logs fired but the reload handlers never
/// ran. Keep `method` here as the un-prefixed name; the prefix is a wire
/// detail added in one place.
fn internal_reload_request_line(id: &str, method: &str, params: serde_json::Value) -> String {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": format!("_{method}"),
        "params": params,
    });
    format!("{}\n", msg)
}

/// Start a skills file watcher and wire it to inject `grow/internal/reload_skills`
/// messages into the shared ACP incoming stream when SKILL.md files change on disk.
///
/// or `None` if no directories could be watched.
fn spawn_skills_file_watcher<W>(
    acp_incoming_tx: &Arc<TokioMutex<W>>,
    skills_paths: &[String],
) -> Option<tokio::task::JoinHandle<()>>
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let cwd = std::env::current_dir().unwrap_or_default();
    let workspace_user_dir = agent::prompt::workspace_user::optional_workspace_user_dir();
    let (mut watcher, mut skills_rx) = crate::config::watcher::SkillsFileWatcher::start(
        Some(cwd.as_path()),
        workspace_user_dir.as_deref(),
        skills_paths,
    )?;
    let skills_tx = acp_incoming_tx.clone();
    let task = tokio::spawn(async move {
        while let Some(change) = skills_rx.recv().await {
            let created_discovery_dir = watcher.refresh_new_discovery_dirs();
            let (id, method) = match change {
                crate::config::watcher::DiscoveryChange::Skills if !created_discovery_dir => {
                    info!("Skill directory changed on disk, reloading skills for all sessions");
                    ("skills-reload", "grow/internal/reload_skills")
                }
                crate::config::watcher::DiscoveryChange::Skills => {
                    info!("Discovery directory created on disk, reloading skills and workflows");
                    ("skills-reload", "grow/internal/reload_skills")
                }
                crate::config::watcher::DiscoveryChange::Workflows => {
                    info!(
                        "Workflow directory changed on disk, re-advertising commands for all sessions"
                    );
                    ("workflows-reload", "grow/internal/reload_workflows")
                }
            };
            let line = internal_reload_request_line(id, method, serde_json::json!({}));
            let mut tx = skills_tx.lock().await;
            if let Err(e) = tx.write_all(line.as_bytes()).await {
                warn!(
                    error = %e,
                    "failed to inject skills reload into ACP stream"
                );
            }
        }
    });
    Some(task)
}

/// Register the process-lifetime runtime so shared filesystem watchers
/// ([`fsnotify::shared`]) run their event loops on a runtime that outlives
/// individual sessions (each session builds its own short-lived runtime).
/// Idempotent — safe to call from every agent entrypoint.
fn register_fs_watch_runtime() {
    fsnotify::set_runtime_handle(tokio::runtime::Handle::current());
}

pub async fn run_stdio_agent(
    agent_config: &AgentConfig,
    memory_config: Option<crate::config::MemoryConfig>,
) -> anyhow::Result<()> {
    register_fs_watch_runtime();
    // A stdio agent is a protocol child speaking over pipes inherited from
    // whoever spawned it (grow-desktop, IDE clients, the agent SDKs, a parent
    // agent's subagent harness) — it is useless without that parent. stdin
    // EOF already triggers shutdown below, but an agent wedged mid-turn (or
    // under thread exhaustion) may never read stdin again; bind to parent
    // death (Linux `PR_SET_PDEATHSIG(SIGTERM)`, no-op elsewhere) so the
    // kernel reaps it instead of leaving an orphan accumulating pid slots on
    // shared hosts. The leader entrypoint intentionally does NOT do this —
    // it is designed to outlive its clients.
    if let Err(error) = tty_utils::kill_current_process_on_parent_death() {
        tracing::warn!(
            %error,
            "failed to bind to parent death; agent will not die with its \
             parent — stdin EOF remains the only cleanup"
        );
    }
    // Stamp binary version into unified log entries so zombie processes
    // are identifiable by version in diagnostic logs.
    ::diagnostics::unified_log::set_version(version::VERSION);

    // Log the embedding client that launched `grow agent stdio`, when provided.
    // This appears early in unified.jsonl and is extremely useful for auth diagnostics.
    if let Ok(version) = std::env::var("GROW_CLIENT_VERSION") {
        crate::unified_log::info(
            "GROW_CLIENT_VERSION",
            None,
            Some(serde_json::json!({ "version": version })),
        );
    }

    let _total_timer = crate::instrumentation_timer!("startup.stdio_agent_total");
    let outgoing = tokio::io::stdout().compat_write();
    // Non-blocking boot: catalog refreshes in the background, not before readiness.
    let agent_config = agent_config.clone();

    // Use a simplex intermediary between stdin and the agent so we can
    // inject internal messages (e.g. skill-reload) alongside real client
    // input. This mirrors the pattern used by `run_leader`.
    let (acp_incoming_rx, acp_incoming_tx) = simplex(MAX_BUFFER_SIZE);
    let incoming = acp_incoming_rx.compat();
    let acp_incoming_tx = Arc::new(TokioMutex::new(acp_incoming_tx));

    // Bridge stdin to the simplex writer. A dedicated OS thread does the
    // blocking stdin reads (see `acp_transport::spawn_stdin_line_reader`): on
    // Windows `tokio::io::stdin()` only delivers buffered lines from a
    // redirected pipe at EOF, so a persistent ACP client (which keeps stdin
    // open) would hang the `initialize` handshake. The forwarder writes each
    // complete line to the simplex so injected internal messages (from the
    // skills watcher) never interleave mid-line with client data.
    let stdin_tx = acp_incoming_tx.clone();
    let (stdin_closed_tx, stdin_closed_rx) = tokio::sync::oneshot::channel();
    let mut stdin_lines = acp_transport::spawn_stdin_line_reader();
    tokio::spawn(async move {
        while let Some(line) = stdin_lines.recv().await {
            let mut tx = stdin_tx.lock().await;
            if tx.write_all(&line).await.is_err() {
                break;
            }
        }
        // Signal that stdin closed. The actual simplex shutdown is performed
        // on the LocalSet so pending ACP request handlers can flush their
        // responses first (they run on the same LocalSet and would be
        // starved by an immediate cross-thread shutdown).
        let _ = stdin_closed_tx.send(());
    });

    let _skills_watcher = spawn_skills_file_watcher(&acp_incoming_tx, &agent_config.skills.paths);

    let local_set = tokio::task::LocalSet::new();
    let result = local_set
        .run_until(async move {
            // Shut down the simplex writer on the LocalSet so it's cooperative with ACP handlers.
            let simplex_tx = acp_incoming_tx;
            tokio::task::spawn_local(async move {
                let _ = stdin_closed_rx.await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                let mut tx = simplex_tx.lock().await;
                let _ = tx.shutdown().await;
            });

            let handle_io = spawn_agent_local(agent_config, memory_config, outgoing, incoming);
            handle_io.await?;
            Ok::<(), anyhow::Error>(())
        })
        .await;
    // Kill PTY child processes so they don't outlive the agent.
    crate::terminal::pty_session::close_all().await;

    result
}

/// Run the local shared agent leader, accepting ACP connections over IPC.
///
/// Startup sequence (lock-then-socket):
/// 1. Acquire the leader flock FIRST — bail if another process holds it.
/// 2. Socket cleanup, channel + readiness-watch creation.
/// 3. IPC server started (`tokio::spawn`) — socket bound HERE, before auth.
/// 4. Wait for socket to appear (fast: < 100 ms).
/// 5. Lock handoff with spawner (if launched via connect_or_spawn).
/// 6. Bootstrap the explicitly configured model catalog.
/// 7. `ready_tx.send(true)` — unblocks ACP forwarding in the IPC server.
/// 8. LocalSet: agent, IPC bridges, and config watcher.
///
/// # Arguments
///
/// * `agent_config` - The agent configuration
/// * `no_exit_on_disconnect` - If true, the leader will not exit when all clients disconnect
pub async fn run_leader(
    agent_config: &AgentConfig,
    no_exit_on_disconnect: bool,
    auto_update_check: Option<LeaderAutoUpdateConfig>,
    memory_config: Option<crate::config::MemoryConfig>,
) -> anyhow::Result<()> {
    use crate::leader::{
        LeaderLock, LeaderServerControlState, LeaderServerMetadata, LockError, ShutdownReason,
        run_leader_server,
    };
    use tokio::sync::watch;
    use tokio_util::sync::CancellationToken;

    register_fs_watch_runtime();
    ::diagnostics::unified_log::set_version(version::VERSION);

    let mut agent_config = agent_config.clone();
    agent_config.mode = crate::agent::config::AgentMode::Leader;

    let mut lock = LeaderLock::new();
    let socket_path = lock.socket_path().clone();

    // ── Phase 1: Acquire the leader flock FIRST (lock-then-socket) ────────────
    //
    // SINGLE-LEADER INVARIANT: only the flock holder may create/remove the socket
    // and it holds the flock for its whole lifetime, so a racing leader can never
    // clobber a live socket.
    match lock.try_acquire() {
        Ok(true) => {
            lock.write_pid()?;
            debug!("Acquired leader lock, proceeding as leader");
        }
        Ok(false) => {
            // Fast path: a fully-running leader (flock held AND socket bound) →
            // exit so the client adopts it.
            if crate::leader::listener_is_ready(&socket_path) {
                info!(
                    "Another process holds the leader lock with a bound socket ({}). \
                     Exiting so the client adopts it.",
                    socket_path.display()
                );
                return Err(anyhow::anyhow!(
                    "Another leader already holds the lock at {}",
                    socket_path.display()
                ));
            }

            // Held but no socket yet: a spawner is mid-handoff, or an old-flow
            // client holds the flock across its spawn window. Wait (re-opening the
            // path each poll to tolerate the old client's Drop unlinking the inode)
            // before conceding.
            match lock.acquire_reopen_timeout(LEADER_ACQUIRE_TIMEOUT).await {
                Ok(()) => {
                    lock.write_pid()?;
                    debug!("Acquired leader lock after bounded wait, proceeding as leader");
                }
                Err(LockError::Timeout(_)) => {
                    info!(
                        "Timed out waiting for the leader lock ({}). Exiting so the \
                         client adopts whoever won it.",
                        socket_path.display()
                    );
                    return Err(anyhow::anyhow!(
                        "Timed out acquiring leader lock at {}",
                        socket_path.display()
                    ));
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to acquire leader lock: {}", e)),
            }
        }
        Err(e) => return Err(anyhow::anyhow!("Failed to acquire leader lock: {}", e)),
    }

    // ── Phase 2: Clean up stale socket (we hold the flock, so this is safe) ────
    lock.cleanup_socket()?;
    info!("Leader server starting");

    // ── Phase 3: Create all channels + readiness watch ────────────────────────
    //
    // All channels are created here so the IPC server can start receiving
    // client connections immediately, before auth/prefetch begin.

    // IPC ↔ agent channels
    let (ipc_to_agent_tx, mut ipc_to_agent_rx) = mpsc::unbounded_channel::<String>();
    let (agent_to_ipc_tx, agent_to_ipc_rx) = mpsc::unbounded_channel::<String>();

    // ACP simplex streams for the agent connection
    let (acp_incoming_rx, acp_incoming_tx) = simplex(MAX_BUFFER_SIZE);
    let (acp_outgoing_rx, acp_outgoing_tx) = simplex(MAX_BUFFER_SIZE);

    let incoming = acp_incoming_rx.compat();
    let outgoing = acp_outgoing_tx.compat_write();

    // Shared writer used by the IPC bridge and local config watcher.
    let acp_incoming_tx = Arc::new(TokioMutex::new(acp_incoming_tx));

    // Cancellation token for the entire leader lifetime.
    let cancel = CancellationToken::new();

    // Readiness watch: IPC server gates ACP forwarding until this is `true`.
    // We hold `ready_tx` here and send `true` after auth + prefetch succeed.
    let (ready_tx, ready_rx) = watch::channel(false);

    // Shutdown-reason watch: default is Manual; the auto-update checker and the
    // leader's `RelaunchForUpdate` control handler send AutoUpdate before
    // cancelling so clients receive the correct ShuttingDown reason. The server
    // derives its own receiver from the sender via `subscribe()`, so we only need
    // to keep the sender; `_shutdown_reason_rx` is held to keep the channel open.
    let (shutdown_tx, _shutdown_reason_rx) = watch::channel(ShutdownReason::Manual);

    let client_count = Arc::new(AtomicUsize::new(0));
    let agent_busy = Arc::new(AtomicBool::new(false));
    // Agent-derived activity view for auto-update and graceful relaunch.
    let agent_activity = crate::agent::activity::AgentActivity::default();
    let control_state = LeaderServerControlState::new(LeaderServerMetadata {
        pid: std::process::id(),
        socket_path: socket_path.clone(),
        lock_path: lock.lock_path().clone(),
        leader_binary_version: version::VERSION.to_string(),
    });

    // ── Phase 4: Bind socket and start IPC server (BEFORE auth/prefetch) ──────
    //
    // Starting the server here means connect_or_spawn sees the socket in < 100 ms
    // regardless of how long auth + model prefetch take. The `ready_rx` gate inside
    // the server ensures early ACP messages get a structured `leader_starting` error
    // rather than hanging or silently dropping.
    let ipc_server_cancel = cancel.clone();
    let socket_path_for_server = socket_path.clone();
    let client_count_for_server = client_count.clone();
    let agent_busy_for_server = agent_busy.clone();
    let agent_activity_for_server = agent_activity.clone();
    let shutdown_tx_for_server = shutdown_tx.clone();
    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = run_leader_server(
            socket_path_for_server,
            ipc_to_agent_tx,
            agent_to_ipc_rx,
            ipc_server_cancel,
            no_exit_on_disconnect,
            client_count_for_server,
            agent_busy_for_server,
            agent_activity_for_server,
            ready_rx,
            shutdown_tx_for_server,
            None, // use LEADER_VERSION constant
            control_state,
        )
        .await
        {
            warn!(error = ?e, "Leader server error");
        }
    });

    // ── Phase 5: Wait for socket to appear (fast: < 100 ms now) ──────────────
    let socket_ready_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !crate::leader::listener_is_ready(&socket_path) {
        if tokio::time::Instant::now() >= socket_ready_deadline {
            cancel.cancel();
            return Err(anyhow::anyhow!(
                "Timeout waiting for IPC socket to be created"
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    debug!("IPC socket created");

    // Keep `lock` alive so its `Drop` removes the lock + socket on exit.
    let _lock = lock;

    // ── Phase 6: local bootstrap ─────────────────────────────────────────────
    let remote_settings: Option<_> = None;

    // ── Phase 7: Signal readiness ─────────────────────────────────────────────
    //
    // Unblocks ACP forwarding inside the IPC server. From this point on, client
    // ACP messages are forwarded to the agent as normal.
    let _ = ready_tx.send(true);
    info!(
        "Leader ready: local-only boot (model/settings refresh runs in background), ACP forwarding enabled"
    );

    // ── Phase 8: LocalSet — agent, IPC bridges, config watcher ───────────────

    let local_set = tokio::task::LocalSet::new();
    let mut agent_config_for_spawn = agent_config.clone();
    agent_config_for_spawn.remote_settings = remote_settings;
    crate::util::config::sync_campaign_fields(&mut agent_config_for_spawn);
    let agent_to_ipc_tx_clone = agent_to_ipc_tx.clone();
    let cancel_clone = cancel.clone();

    let (agent_config_for_spawn, shared_models_manager) =
        bootstrap(&agent_config_for_spawn).unwrap_or_else(exit_on_config_error);

    let models_manager_for_agent = shared_models_manager.clone();
    let models_manager_for_config = shared_models_manager;

    // Resolve `mcp.recursive_config_watch`
    // ONCE here, before the channel is created, so a kill-switch
    // value of `false` skips channel construction entirely. Previously
    // the channel was always created and `tx` always installed on
    // the agent; the drain task only ran when the flag was on, so
    // every `notify_session_cwd_for_watch` call leaked a `PathBuf`
    // into a never-drained channel.
    let recursive_config_watch_enabled = {
        let user_cfg = crate::config::load_from_disk().ok();
        let requirements = crate::agent::config::read_requirements_toml();
        crate::util::config::resolve_mcp_recursive_config_watch(
            requirements.as_ref(),
            user_cfg.as_ref(),
            /* managed */ None,
        )
    };

    local_set
        .run_until(async move {
            // Channel for fanning new session cwds from
            // the agent (each `spawn_and_register_session` call) into
            // the leader's `ConfigFileWatcher::watch_path`. Both ends
            // live inside the `LocalSet` so neither needs `Send`. The
            // tx is installed on the agent before `AgentSideConnection`
            // moves it; the rx is drained by a small task spawned
            // alongside the watcher below.
            //
            // Only create the channel when the kill-
            // switch is `true`. With the flag off,
            // `notify_session_cwd_for_watch` becomes a no-op (no
            // `tx` installed) and no memory leaks regardless of how
            // many sessions spawn over the leader's lifetime.
            let (config_watcher_path_tx, config_watcher_path_rx_opt) =
                if recursive_config_watch_enabled {
                    let (tx, rx) = mpsc::unbounded_channel::<std::path::PathBuf>();
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
            let mut config_watcher_path_rx = config_watcher_path_rx_opt;

            // Spawn the agent
            let agent_config_watcher_path_tx = config_watcher_path_tx.clone();
            let agent_activity_for_agent = agent_activity.clone();
            tokio::task::spawn_local(async move {
                let (gw_tx, gw_rx) = tokio::sync::mpsc::unbounded_channel();
                let gateway = GatewaySender::new(gw_tx);
                let mut agent = MvpAgent::with_models(
                    gateway,
                    &agent_config_for_spawn,
                    models_manager_for_agent,
                );
                agent.set_activity(agent_activity_for_agent);
                if let Some(mc) = memory_config {
                    agent.set_memory_config(mc);
                }
                if let Some(tx) = agent_config_watcher_path_tx {
                    agent.set_config_watcher_path_tx(tx);
                }
                let incoming = LineBufferedRead::spawn_local(incoming);
                let (conn, handle_io) =
                    acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                        tokio::task::spawn_local(fut);
                    });
                tokio::task::spawn_local(
                    GatewayReceiver::new(gw_rx, conn).run(),
                );

                if let Err(e) = handle_io.await {
                    warn!(error = ?e, "Agent I/O handler error");
                }
                info!("Agent task completed");
            });

            // Bridge IPC messages to agent (from stdio clients)
            let acp_incoming_tx_ipc = acp_incoming_tx.clone();
            tokio::task::spawn_local(async move {
                while let Some(msg) = ipc_to_agent_rx.recv().await {
                    let mut tx = acp_incoming_tx_ipc.lock().await;
                    if tx.write_all(msg.as_bytes()).await.is_err()
                        || tx.write_all(b"\n").await.is_err()
                    {
                        warn!("Failed to write IPC message to agent");
                        break;
                    }
                }
            });

            // Bridge agent responses to local IPC clients.
            tokio::task::spawn_local(async move {
                let mut reader = BufReader::new(acp_outgoing_rx);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let msg = line.trim_end_matches(['\r', '\n']).to_string();
                            if !msg.is_empty() {
                                let _ = agent_to_ipc_tx_clone.send(msg);
                            }
                        }
                        Err(e) => {
                            warn!(error = ?e, "Error reading from agent outgoing stream");
                            break;
                        }
                    }
                }
            });

            // Spawn auto-update checker if configured.
            let update_cancel = cancel_clone.clone();
            if let Some(update_config) = auto_update_check {
                let agent_busy_for_update = agent_busy.clone();
                let agent_activity_for_update = agent_activity.clone();
                let cancel_for_update = cancel_clone.clone();
                tokio::spawn(run_auto_update_checker(
                    update_config,
                    agent_busy_for_update,
                    agent_activity_for_update,
                    cancel_for_update,
                    shutdown_tx,
                ));
            }

            // Config hot-reload watcher
            let cwd_for_watcher = std::env::current_dir().unwrap_or_default();
            let mut watch_paths = crate::config::find_project_configs(&cwd_for_watcher);
            watch_paths.extend(crate::util::config::mcp_json_candidate_paths(
                &cwd_for_watcher,
            ));
            if let Some(home) = dirs::home_dir() {
                watch_paths.push(home.join(".claude.json"));
            }
            let (config_update_tx, mut config_update_rx) =
                mpsc::unbounded_channel::<crate::config::reloader::ConfigUpdate>();

            // `mcp.recursive_config_watch` (default
            // `true`) was resolved above (before the async block) so
            // the per-session-cwd channel could be gated. The
            // watcher passes `Some(cwd)` here only when the flag is
            // on. When disabled, behavior reverts to the prior
            // default: only explicit `extra_paths` are watched (kill
            // switch for the rollout).
            let watcher_cwd = recursive_config_watch_enabled.then_some(cwd_for_watcher.as_path());

            let _config_watcher = if let Some(watcher) =
                crate::config::reloader::start_config_reload(
                    &grow_home::grow_home(),
                    &watch_paths,
                    watcher_cwd,
                    None, // settings stream in after readiness via background refresh
                    config_update_tx,
                    agent_config.cli_experimental_memory,
                    agent_config.cli_no_memory,
                    cancel_clone.clone(),
                ) {
                // Share ownership between the leader's
                // long-lived binding and the per-cwd dynamic
                // registration drain task. `Rc<RefCell<>>` is safe
                // because both ends live inside the leader's
                // `LocalSet` — the watcher type is not `Sync`-needed.
                let watcher = std::rc::Rc::new(std::cell::RefCell::new(watcher));

                // Dynamic registration drain. Lives only
                // when the recursive_config_watch flag is on AND the
                // OS watcher started. With the flag
                // off the channel itself was never created, so
                // there's no rx to drain and no `PathBuf` ever
                // queued (no leak).
                if let Some(mut rx) = config_watcher_path_rx.take() {
                    let cancel_for_drain = cancel_clone.clone();
                    let watcher_for_drain = watcher.clone();
                    tokio::task::spawn_local(async move {
                        loop {
                            tokio::select! {
                                biased;
                                _ = cancel_for_drain.cancelled() => break,
                                cwd = rx.recv() => match cwd {
                                    Some(cwd) => watcher_for_drain.borrow_mut().watch_path(&cwd),
                                    None => break,
                                },
                            }
                        }
                    });
                }
                Some(watcher)
            } else {
                warn!("Config file watcher failed to start; hot-reload disabled");
                None
            };

            let _skills_watcher =
                spawn_skills_file_watcher(&acp_incoming_tx, &agent_config.skills.paths);

            let ipc_tx_for_config = agent_to_ipc_tx.clone();
            let acp_tx_for_config = acp_incoming_tx.clone();
            tokio::task::spawn_local(async move {
                use crate::config::reloader::ConfigUpdate;
                while let Some(update) = config_update_rx.recv().await {
                    match update {
                        ConfigUpdate::McpServersChanged => {
                            info!("MCP server config change detected — reloading active sessions");
                            let line = internal_reload_request_line(
                                "config-reload-mcp",
                                "grow/internal/reload_all_mcp_servers",
                                serde_json::json!({}),
                            );
                            let mut tx = acp_tx_for_config.lock().await;
                            if let Err(e) = tx.write_all(line.as_bytes()).await {
                                warn!(error = %e, "failed to inject MCP reload into ACP stream");
                            }
                        }
                        ConfigUpdate::ProjectMcpServersChanged { cwd } => {
                            // Scope the reload to
                            // sessions whose cwd matches `cwd` (or is
                            // a descendant). The actual filtering
                            // happens in
                            // `handle_reload_project_mcp_servers`
                            // (extensions/session_admin.rs) — this
                            // arm just injects the ACP method with
                            // the cwd as a param.
                            info!(
                                cwd = %cwd.display(),
                                "project MCP config change detected — reloading matching sessions"
                            );
                            let line = internal_reload_request_line(
                                "config-reload-project-mcp",
                                "grow/internal/reload_project_mcp_servers",
                                serde_json::json!({ "cwd": cwd.to_string_lossy() }),
                            );
                            let mut tx = acp_tx_for_config.lock().await;
                            if let Err(e) = tx.write_all(line.as_bytes()).await {
                                warn!(
                                    error = %e,
                                    "failed to inject project MCP reload into ACP stream"
                                );
                            }
                        }
                        ConfigUpdate::ModelsChanged => {
                            info!("Model config change detected — reloading agent model list");
                            let line = internal_reload_request_line(
                                "config-reload-models",
                                "grow/internal/reload_models",
                                serde_json::json!({}),
                            );
                            let mut tx = acp_tx_for_config.lock().await;
                            if let Err(e) = tx.write_all(line.as_bytes()).await {
                                warn!(error = %e, "failed to inject model reload into ACP stream");
                            }
                        }
                        ConfigUpdate::Announcements(announcements) => {
                            info!(count = announcements.len(), "Local announcements changed — updating clients");
                            let line = internal_reload_request_line(
                                "config-reload-announcements",
                                "grow/internal/reload_announcements",
                                serde_json::json!({ "announcements": announcements }),
                            );
                            let mut tx = acp_tx_for_config.lock().await;
                            if let Err(e) = tx.write_all(line.as_bytes()).await {
                                warn!(error = %e, "failed to inject announcements reload into ACP stream");
                            }
                        }
                        ConfigUpdate::Memory(mem) => {
                            info!(
                                enabled = mem.enabled,
                                "Memory config change detected by watcher"
                            );
                        }
                        ConfigUpdate::Skills(skills) => {
                            info!(
                                paths = skills.paths.len(),
                                "Skills config change detected by watcher"
                            );
                        }
                        ConfigUpdate::Compat(_compat) => {
                            info!(
                                "Compat config change detected by watcher \
                                 (applies on next agent rebuild)"
                            );
                        }
                        ConfigUpdate::Ui {
                            theme,
                            yolo,
                            fork_secondary_model,
                        } => {
                            info!("UI config change detected by watcher");
                            let notification = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "grow/config_changed",
                                "params": {
                                    "section": "ui",
                                    "changes": {
                                        "theme": theme,
                                        "yolo": yolo,
                                        "fork_secondary_model": fork_secondary_model,
                                    }
                                }
                            });
                            let _ = ipc_tx_for_config.send(notification.to_string());
                        }
                    }
                }
            });

            // Wait for IPC server shutdown or cancellation.
            // ipc_handle is a JoinHandle from tokio::spawn — awaitable directly.
            tokio::select! {
                biased;
                _ = ipc_handle => {
                    info!("IPC server stopped, shutting down leader");
                }
                _ = update_cancel.cancelled() => {
                    info!("Leader cancelled");
                }
            }

            anyhow::Ok(())
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use tokio::sync::watch;
    use tokio_util::sync::CancellationToken;

    /// Create a throwaway shutdown_tx for tests that don't care about the reason.
    fn dummy_shutdown_tx() -> watch::Sender<crate::leader::ShutdownReason> {
        watch::channel(crate::leader::ShutdownReason::Manual).0
    }

    /// Helper: build a LeaderAutoUpdateConfig whose check_fn always returns the given value.
    fn always_config(update_available: bool) -> LeaderAutoUpdateConfig {
        LeaderAutoUpdateConfig {
            check_interval: Duration::from_millis(10),
            check_fn: Box::new(move || Box::pin(async move { update_available })),
        }
    }

    /// Helper: build a LeaderAutoUpdateConfig that returns `false` for the first
    /// `skip` calls, then `true` for all subsequent calls.
    fn delayed_update_config(skip: u32) -> LeaderAutoUpdateConfig {
        let counter = Arc::new(AtomicU32::new(0));
        LeaderAutoUpdateConfig {
            check_interval: Duration::from_millis(10),
            check_fn: Box::new(move || {
                let counter = counter.clone();
                Box::pin(async move {
                    let n = counter.fetch_add(1, Ordering::Relaxed);
                    n >= skip
                })
            }),
        }
    }

    #[test]
    fn internal_reload_request_line_uses_wire_ext_prefix() {
        let line = internal_reload_request_line(
            "config-reload-models",
            "grow/internal/reload_models",
            serde_json::json!({}),
        );
        assert!(line.ends_with('\n'), "must be a newline-terminated line");
        let msg: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(
            msg["method"], "_grow/internal/reload_models",
            "wire method must carry the `_` ext prefix or the ACP decoder \
             rejects it with method_not_found"
        );
        assert_eq!(msg["id"], "config-reload-models");
        assert_eq!(msg["jsonrpc"], "2.0");

        // Params must pass through verbatim (project-MCP reload carries cwd).
        let line = internal_reload_request_line(
            "config-reload-project-mcp",
            "grow/internal/reload_project_mcp_servers",
            serde_json::json!({ "cwd": "/repo/x" }),
        );
        let msg: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(msg["params"]["cwd"], "/repo/x");
    }

    #[tokio::test]
    async fn auto_update_cancels_when_update_available_and_agent_idle() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();

        let config = always_config(true);

        // The checker should cancel the token on its first check (agent idle)
        tokio::time::timeout(
            Duration::from_secs(2),
            run_auto_update_checker(
                config,
                agent_busy,
                crate::agent::activity::AgentActivity::default(),
                cancel.clone(),
                dummy_shutdown_tx(),
            ),
        )
        .await
        .expect("checker should complete within timeout");

        assert!(cancel.is_cancelled(), "cancel token should be triggered");
    }

    #[tokio::test]
    async fn auto_update_defers_when_agent_busy() {
        let agent_busy = Arc::new(AtomicBool::new(true)); // agent is processing a prompt
        let cancel = CancellationToken::new();

        let config = delayed_update_config(0); // always returns true

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Wait enough for multiple checks to fire
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Token should NOT be cancelled (agent is busy)
        assert!(
            !cancel_clone.is_cancelled(),
            "cancel token should NOT be triggered when agent is busy"
        );

        // Clean up
        cancel_clone.cancel();
        let _ = checker.await;
    }

    #[tokio::test]
    async fn auto_update_no_cancel_when_no_update_available() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();

        let config = always_config(false);

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Let several checks fire
        tokio::time::sleep(Duration::from_millis(80)).await;

        assert!(
            !cancel_clone.is_cancelled(),
            "cancel token should NOT be triggered when no update is available"
        );

        // Clean up
        cancel_clone.cancel();
        let _ = checker.await;
    }

    #[tokio::test]
    async fn auto_update_cancels_after_agent_becomes_idle() {
        let agent_busy = Arc::new(AtomicBool::new(true)); // agent processing initially
        let cancel = CancellationToken::new();

        // Update is always available, but agent is busy initially
        let config = always_config(true);

        let agent_busy_clone = agent_busy.clone();
        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Let a few checks fire while agent is busy
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !cancel_clone.is_cancelled(),
            "should not cancel while agent is busy"
        );

        // Simulate agent finishing its work (prompt completes)
        agent_busy_clone.store(false, Ordering::Relaxed);

        // Wait for the next check to fire and trigger cancellation
        tokio::time::timeout(Duration::from_secs(2), checker)
            .await
            .expect("checker should complete within timeout")
            .expect("checker task should not panic");

        assert!(
            cancel_clone.is_cancelled(),
            "cancel token should be triggered after agent becomes idle"
        );
    }

    #[tokio::test]
    async fn auto_update_stops_when_externally_cancelled() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();

        // No update available, so the checker runs indefinitely
        let config = always_config(false);

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Cancel externally
        cancel_clone.cancel();

        // Checker should exit promptly
        tokio::time::timeout(Duration::from_secs(2), checker)
            .await
            .expect("checker should exit within timeout after external cancel")
            .expect("checker task should not panic");
    }

    #[tokio::test]
    async fn auto_update_calls_check_fn_multiple_times() {
        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        let agent_busy = Arc::new(AtomicBool::new(true)); // agent busy, so it defers
        let cancel = CancellationToken::new();

        let config = LeaderAutoUpdateConfig {
            check_interval: Duration::from_millis(10),
            check_fn: Box::new(move || {
                let cc = call_count_clone.clone();
                Box::pin(async move {
                    cc.fetch_add(1, Ordering::Relaxed);
                    true // update always available, but won't cancel because agent is busy
                })
            }),
        };

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Let several checks fire. Use a generous timeout to avoid flakiness
        // in CI where the first check may take longer due to task scheduling.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let calls = call_count.load(Ordering::Relaxed);
        assert!(
            calls >= 2,
            "check_fn should have been called multiple times, got {}",
            calls
        );

        cancel_clone.cancel();
        let _ = checker.await;
    }

    #[tokio::test]
    async fn auto_update_cancels_during_hanging_check_fn() {
        // Simulates a stalled-HTTP scenario: check_fn hangs (stalled HTTP).
        // The checker should still respond to cancellation thanks to the select!.
        let agent_busy = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();

        let config = LeaderAutoUpdateConfig {
            check_interval: Duration::from_millis(10),
            check_fn: Box::new(|| {
                Box::pin(async {
                    // Simulate a hanging HTTP call that never completes
                    futures::future::pending::<bool>().await
                })
            }),
        };

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            crate::agent::activity::AgentActivity::default(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        // Let the checker enter the hanging check_fn
        tokio::time::sleep(Duration::from_millis(30)).await;

        // Cancel externally — should NOT hang
        cancel_clone.cancel();

        // Checker must exit promptly despite the hanging check_fn
        tokio::time::timeout(Duration::from_secs(2), checker)
            .await
            .expect("checker should exit within timeout even with hanging check_fn")
            .expect("checker task should not panic");
    }

    /// The IPC `agent_busy` flag does not cover work after request dispatch — the checker
    /// must also defer on the agent-derived activity signal (running turn,
    /// pending interaction, or live subagent).
    #[tokio::test]
    async fn auto_update_defers_when_agent_activity_busy() {
        let agent_busy = Arc::new(AtomicBool::new(false)); // IPC view: idle
        let activity = crate::agent::activity::AgentActivity::default();
        // Agent view: a subagent is still running after request dispatch.
        activity.subagent_gauge().store(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();

        let config = always_config(true); // update always "installed"

        let cancel_clone = cancel.clone();
        let checker = tokio::spawn(run_auto_update_checker(
            config,
            agent_busy,
            activity.clone(),
            cancel.clone(),
            dummy_shutdown_tx(),
        ));

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            !cancel_clone.is_cancelled(),
            "must not shut down while the agent (not IPC) is busy"
        );

        // Subagent finishes → next tick shuts down.
        activity.subagent_gauge().store(0, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_secs(2), checker)
            .await
            .expect("checker should complete within timeout")
            .expect("checker task should not panic");
        assert!(cancel_clone.is_cancelled());
    }

    /// A permanently-busy signal must not pin the leader to an old binary
    /// forever: after MAX_AUTO_UPDATE_BUSY_DEFERRALS the update proceeds.
    #[tokio::test]
    async fn auto_update_forces_shutdown_after_deferral_limit() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let activity = crate::agent::activity::AgentActivity::default();
        // Permanently busy (e.g. an orphaned parked interaction).
        activity.subagent_gauge().store(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();

        let config = always_config(true); // update always "installed"

        // 10ms interval × (24 deferrals + 1) ≈ 250ms — well within timeout.
        tokio::time::timeout(
            Duration::from_secs(10),
            run_auto_update_checker(
                config,
                agent_busy,
                activity,
                cancel.clone(),
                dummy_shutdown_tx(),
            ),
        )
        .await
        .expect("checker should force shutdown after the deferral limit");
        assert!(cancel.is_cancelled());
    }

    /// Before cancelling (which drops the LocalSet and aborts session actors),
    /// the checker must ask every registered session actor to shut down and
    /// wait for it to exit, so buffered state is flushed to disk.
    #[tokio::test]
    async fn auto_update_flushes_sessions_before_cancel() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let activity = crate::agent::activity::AgentActivity::default();
        let (mut cmd_rx, _prompt_id, _pending) = activity.register_for_test("s1");
        let cancel = CancellationToken::new();

        // Simulated session actor: records the Shutdown command, then exits
        // (dropping cmd_rx, which is how the flush observes completion).
        let got_shutdown = Arc::new(AtomicBool::new(false));
        let got_shutdown_clone = got_shutdown.clone();
        let cancel_for_actor = cancel.clone();
        let actor = tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if matches!(cmd, crate::session::SessionCommand::Shutdown) {
                    assert!(
                        !cancel_for_actor.is_cancelled(),
                        "session flush must happen BEFORE the leader is cancelled"
                    );
                    got_shutdown_clone.store(true, Ordering::Relaxed);
                    return;
                }
            }
        });

        let config = always_config(true);
        tokio::time::timeout(
            Duration::from_secs(2),
            run_auto_update_checker(
                config,
                agent_busy,
                activity,
                cancel.clone(),
                dummy_shutdown_tx(),
            ),
        )
        .await
        .expect("checker should complete within timeout");

        assert!(cancel.is_cancelled());
        actor.await.expect("actor should exit cleanly");
        assert!(
            got_shutdown.load(Ordering::Relaxed),
            "session actor must receive SessionCommand::Shutdown before leader cancel"
        );
    }

    /// Verify that when an update is installed and the agent is idle, the checker
    /// sends `ShutdownReason::AutoUpdate` via the `shutdown_tx` channel BEFORE
    /// cancelling the token, so the IPC server broadcasts the correct reason.
    #[tokio::test]
    async fn auto_update_sets_shutdown_reason_auto_update() {
        let agent_busy = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(crate::leader::ShutdownReason::Manual);

        let config = always_config(true); // update always available

        tokio::time::timeout(
            Duration::from_secs(2),
            run_auto_update_checker(
                config,
                agent_busy,
                crate::agent::activity::AgentActivity::default(),
                cancel.clone(),
                shutdown_tx,
            ),
        )
        .await
        .expect("checker should complete within timeout");

        assert!(cancel.is_cancelled(), "cancel token should be triggered");

        // The shutdown_tx must have been updated to AutoUpdate before cancel fired.
        shutdown_rx.mark_changed(); // ensure borrow sees latest value
        assert_eq!(
            *shutdown_rx.borrow(),
            crate::leader::ShutdownReason::AutoUpdate,
            "shutdown reason must be AutoUpdate for an auto-update-triggered shutdown"
        );
    }
}
