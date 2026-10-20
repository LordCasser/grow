//! Agent spawning — creates the agent process and ACP channels.
//!
//! Simplified to only support GrowShell (in-process) mode.
//! Subprocess and remote modes can be added later if needed.

use std::io::IsTerminal;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use acp_transport::protocol as acp;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

use acp_transport::{
    AcpAgentChannel, AcpClientChannel, AcpClientTx, AcpGatewayReceiver, AcpGatewaySender,
    acp_channels,
};
use shell::agent::{MvpAgent, activity::SESSION_FLUSH_GRACE, config::Config as AgentConfig};

/// Extra slack when joining the agent OS thread after cancel so the flush
/// can finish and the thread can unwind.
const AGENT_JOIN_SLACK: Duration = Duration::from_secs(2);

/// How long the join stays silent before telling an interactive user why exit
/// is taking a moment. Short joins (the common case) print nothing.
const JOIN_NOTICE_AFTER: Duration = Duration::from_millis(1500);

/// Result of spawning a child agent.
pub struct SpawnedAgent {
    /// Agent worker OS thread. Hand to [`AgentShutdownGuard`] so the worker is
    /// cancelled and joined — letting session actors flush SessionEnd hooks —
    /// on every exit path.
    pub thread_handle: thread::JoinHandle<Result<()>>,
    pub channel: AcpClientChannel,
    pub cancel: CancellationToken,
}

/// The single teardown mechanism for an in-process agent: cancels the worker
/// and joins it on drop, so session actors always get
/// `SessionCommand::Shutdown` (SessionEnd hooks, memory save) before the
/// process exits — on normal return, `?` bail, or panic unwind alike.
///
/// Hold one from every site that calls [`spawn_shell`] (headless, the TUI,
/// `models`, `worktree`, `share`). Scope-end drop is the default; the TUI is the
/// one caller that drops it explicitly, because the join has to happen before
/// background processes are reaped (see `app::run`).
pub struct AgentShutdownGuard {
    cancel: CancellationToken,
    thread: Option<thread::JoinHandle<Result<()>>>,
}

impl AgentShutdownGuard {
    /// Guard an in-process agent worker. A `None` thread makes the guard a
    /// no-op cancel (leader mode has no in-process worker to join).
    pub fn new(cancel: CancellationToken, thread: Option<thread::JoinHandle<Result<()>>>) -> Self {
        Self { cancel, thread }
    }
}

impl Drop for AgentShutdownGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        let Some(handle) = self.thread.take() else {
            return;
        };
        let timeout = SESSION_FLUSH_GRACE + AGENT_JOIN_SLACK;
        match join_agent_thread(handle, timeout) {
            JoinOutcome::Joined => {}
            JoinOutcome::Failed(error) => {
                tracing::warn!(%error, "agent worker exited with error after cancel");
            }
            JoinOutcome::Panicked(panic) => {
                tracing::warn!(%panic, "agent worker panicked after cancel");
            }
            JoinOutcome::TimedOut => {
                tracing::warn!(
                    timeout_ms = timeout.as_millis() as u64,
                    "agent worker did not exit within grace after cancel; \
                     session hooks may be incomplete"
                );
            }
            JoinOutcome::HelperLost => {
                tracing::warn!("agent worker join helper disappeared; proceeding");
            }
        }
    }
}

/// Why the join ended, so each case is explicit at the call site (and callers
/// can tell a completed flush from an abandoned one).
#[derive(Debug, PartialEq, Eq)]
enum JoinOutcome {
    /// Worker returned cleanly: session actors flushed within the grace.
    Joined,
    /// Worker returned an error; the flush may be incomplete.
    Failed(String),
    /// Worker panicked, with the payload rendered as text.
    Panicked(String),
    /// Worker was still running when the budget elapsed.
    TimedOut,
    /// The join helper vanished without reporting (helper thread itself died).
    HelperLost,
}

/// Wait up to `timeout` for a cancelled agent worker to exit.
///
/// The blocking `join` runs on a helper thread so this stays callable from
/// `Drop` — which cannot await — while every caller sits on the async runtime.
/// On timeout that helper is abandoned rather than joined; this is safe **only
/// because every caller is on its way out of the process**, so the OS reaps the
/// thread at exit. Do not reuse this outside teardown.
fn join_agent_thread(handle: thread::JoinHandle<Result<()>>, timeout: Duration) -> JoinOutcome {
    use std::sync::mpsc::RecvTimeoutError;

    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(handle.join());
    });

    // Two-phase wait: silent for a short join (overwhelmingly the common case),
    // then a one-line notice so a slow SessionEnd hook does not look like a
    // frozen exit. Only for a terminal — piped/JSON consumers stay clean.
    let quiet = timeout.min(JOIN_NOTICE_AFTER);
    match rx.recv_timeout(quiet) {
        Ok(result) => return classify_join(result),
        Err(RecvTimeoutError::Timeout) => {
            if std::io::stderr().is_terminal() {
                eprintln!("Finishing session hooks…");
            }
        }
        Err(RecvTimeoutError::Disconnected) => return JoinOutcome::HelperLost,
    }
    match rx.recv_timeout(timeout.saturating_sub(quiet)) {
        Ok(result) => classify_join(result),
        Err(RecvTimeoutError::Timeout) => JoinOutcome::TimedOut,
        Err(RecvTimeoutError::Disconnected) => JoinOutcome::HelperLost,
    }
}

fn classify_join(result: thread::Result<Result<()>>) -> JoinOutcome {
    match result {
        Ok(Ok(())) => JoinOutcome::Joined,
        Ok(Err(e)) => JoinOutcome::Failed(e.to_string()),
        Err(payload) => JoinOutcome::Panicked(panic_message(payload)),
    }
}

/// Render a panic payload as text — `panic!` payloads are `&str` or `String`,
/// so the log shows the message instead of an opaque `Any`.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Spawn a GrowShell agent in a background thread.
///
/// Returns the ACP client channel for communication and a cancellation token.
pub async fn spawn_shell(
    agent_config: AgentConfig,
    cancel: &CancellationToken,
    memory_config: Option<shell::config::MemoryConfig>,
) -> Result<SpawnedAgent> {
    // Run the full bootstrap sequence: config resolution, process-level
    // singletons, and model catalog construction.
    let (agent_config, models_manager) =
        shell::agent::init::bootstrap(&agent_config).map_err(|e| anyhow::anyhow!(e))?;

    let agent_cancel = cancel.child_token();
    let (acp_client, acp_agent) = acp_channels();

    let skills_paths = agent_config.skills.paths.clone();
    let experimental_memory = agent_config.cli_experimental_memory;
    let no_memory = agent_config.cli_no_memory;

    let spawn_fn: Box<dyn FnOnce(AcpClientTx) -> Result<MvpAgent> + Send + 'static> = {
        Box::new(move |client_tx| {
            let gateway = AcpGatewaySender::new(client_tx);

            let mut agent = MvpAgent::with_models(gateway, &agent_config, models_manager);
            if let Some(mc) = memory_config {
                agent.set_memory_config(mc);
            }
            Ok(agent)
        })
    };

    // Spawn the agent thread with direct dispatch
    let handle = spawn_agent_thread_direct(
        spawn_fn,
        acp_agent,
        agent_cancel.clone(),
        skills_paths,
        experimental_memory,
        no_memory,
    )?;

    Ok(SpawnedAgent {
        thread_handle: handle,
        channel: acp_client,
        cancel: agent_cancel,
    })
}

/// Use the existing catalog/model extension paths in the in-process worker.
fn config_reload_request(update: shell::config::reloader::ConfigUpdate) -> Option<acp::ExtRequest> {
    use shell::config::reloader::ConfigUpdate;
    let (method, params) = match update {
        ConfigUpdate::ModelsChanged => ("grow/internal/reload_models", serde_json::json!({})),
        ConfigUpdate::McpCatalogChanged { project_root } => (
            "grow/internal/reload_mcp_catalog",
            serde_json::json!({"projectRoot": project_root.map(|root| root.to_string_lossy().into_owned())}),
        ),
        _ => return None,
    };
    Some(acp::ExtRequest::new(
        method,
        serde_json::value::to_raw_value(&params)
            .expect("config reload params serialize")
            .into(),
    ))
}

/// Spawn an agent in a dedicated thread with direct RPC dispatch.
///
/// The agent runs on a single-threaded tokio LocalSet runtime.
/// RPC requests go directly to the agent via Rc, bypassing simplex pipes.
fn spawn_agent_thread_direct(
    spawn_agent: Box<dyn FnOnce(AcpClientTx) -> Result<MvpAgent> + Send + 'static>,
    channel: AcpAgentChannel,
    cancel: CancellationToken,
    skills_paths: Vec<String>,
    experimental_memory: bool,
    no_memory: bool,
) -> Result<thread::JoinHandle<Result<()>>> {
    Ok(thread::Builder::new()
        .name("acp-agent-worker".into())
        .spawn(move || -> Result<()> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let client_tx = channel.tx.clone();
                let mut agent = spawn_agent(client_tx)?;

                // Keep the same catalog reloader and narrow project watches as
                // the leader. Wire cwd registration before accepting sessions.
                let (config_update_tx, mut config_update_rx) =
                    tokio::sync::mpsc::unbounded_channel();
                let project_watch_enabled = shell::util::config::resolve_mcp_recursive_config_watch(
                    shell::config::load_from_disk().ok().as_ref(),
                );
                let cwd = std::env::current_dir().unwrap_or_default();
                let _config_watcher = shell::config::reloader::start_config_reload(
                    &shell::util::grow_home::grow_home(),
                    &[],
                    project_watch_enabled.then_some(cwd.as_path()),
                    None,
                    config_update_tx,
                    experimental_memory,
                    no_memory,
                    cancel.clone(),
                ).map(|watcher| {
                    let watcher = Rc::new(std::cell::RefCell::new(watcher));
                    if project_watch_enabled {
                        let (path_tx, mut path_rx) = tokio::sync::mpsc::unbounded_channel();
                        agent.set_config_watcher_path_tx(path_tx);
                        let watcher_for_paths = watcher.clone();
                        let cancel_paths = cancel.clone();
                        tokio::task::spawn_local(async move {
                            loop {
                                tokio::select! {
                                    biased;
                                    _ = cancel_paths.cancelled() => break,
                                    path = path_rx.recv() => match path {
                                        Some(update) => watcher_for_paths.borrow_mut().apply_session_update(update),
                                        None => break,
                                    },
                                }
                            }
                        });
                    }
                    watcher
                });
                let agent_rc = Rc::new(agent);
                let gw_rx = AcpGatewayReceiver::new(channel.rx, agent_rc.clone()).with_tracing(true);
                tokio::task::spawn_local(gw_rx.run());
                if _config_watcher.is_some() {
                    let agent = agent_rc.clone();
                    let cancel_updates = cancel.clone();
                    tokio::task::spawn_local(async move {
                        loop {
                            let update = tokio::select! {
                                biased;
                                _ = cancel_updates.cancelled() => break,
                                update = config_update_rx.recv() => match update {
                                    Some(update) => update,
                                    None => break,
                                },
                            };
                            let Some(request) = config_reload_request(update) else { continue };
                            if let Err(error) = shell::extensions::session_admin::handle(&agent, &request).await {
                                tracing::warn!(error = ?error, "failed to apply in-process config reload");
                            }
                        }
                    });
                }

                let _skills_watcher = {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let workspace_user_dir =
                        agent::prompt::workspace_user::optional_workspace_user_dir();
                    shell::config::watcher::SkillsFileWatcher::start(
                        Some(cwd.as_path()),
                        workspace_user_dir.as_deref(),
                        &skills_paths,
                    )
                    .map(|(mut watcher, mut skills_rx)| {
                        let agent = agent_rc.clone();
                        tokio::task::spawn_local(async move {
                            while let Some(change) = skills_rx.recv().await {
                                watcher.refresh_new_discovery_dirs();
                                match change {
                                    shell::config::watcher::DiscoveryChange::Skills => {
                                        tracing::info!(
                                            "skill directory changed on disk; reloading skills for all sessions"
                                        );
                                        agent.reload_skills_all_sessions();
                                    }
                                    shell::config::watcher::DiscoveryChange::Workflows => {
                                        tracing::info!(
                                            "workflow directory changed on disk; re-advertising commands for all sessions"
                                        );
                                        agent.advertise_commands_all_sessions();
                                    }
                                }
                            }
                        })
                    })
                };
                tokio::task::yield_now().await;

                // Keep running until cancelled, then flush every live session
                // actor (SessionEnd hooks + memory save) before the LocalSet /
                // agent drop. Session actors live on dedicated OS threads and
                // only exit cleanly on SessionCommand::Shutdown; without this
                // flush, /quit and headless quit race process death and skip
                // SessionEnd. Mirrors leader auto-update / relaunch.
                cancel.cancelled().await;
                agent_rc.flush_all_sessions(SESSION_FLUSH_GRACE).await;
                anyhow::Result::Ok(())
            })
        })?)
}

#[cfg(test)]
mod tests {
    mod mcp_hot_reload;
    use super::*;

    #[test]
    fn join_reports_clean_worker_exit() {
        let handle = thread::spawn(|| Ok(()));
        assert_eq!(
            join_agent_thread(handle, Duration::from_secs(5)),
            JoinOutcome::Joined
        );
    }

    #[test]
    fn join_reports_worker_error() {
        let handle = thread::spawn(|| Err(anyhow::anyhow!("flush failed")));
        assert_eq!(
            join_agent_thread(handle, Duration::from_secs(5)),
            JoinOutcome::Failed("flush failed".to_string())
        );
    }

    /// The timeout branch the built-binary e2e cannot reach: a wedged worker
    /// (e.g. a hung SessionEnd hook) is abandoned once the budget elapses
    /// instead of holding the process open indefinitely.
    #[test]
    fn join_abandons_wedged_worker_at_budget() {
        let handle = thread::spawn(|| {
            thread::sleep(Duration::from_secs(30));
            Ok(())
        });
        let started = std::time::Instant::now();
        assert_eq!(
            join_agent_thread(handle, Duration::from_millis(50)),
            JoinOutcome::TimedOut
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "join must return at its budget, not wait out the worker"
        );
    }

    #[test]
    fn panic_payloads_render_as_text() {
        assert_eq!(
            classify_join(Err(Box::new("boom"))),
            JoinOutcome::Panicked("boom".to_string())
        );
        assert_eq!(
            classify_join(Err(Box::new("boom".to_string()))),
            JoinOutcome::Panicked("boom".to_string())
        );
        assert_eq!(
            classify_join(Err(Box::new(7u32))),
            JoinOutcome::Panicked("non-string panic payload".to_string())
        );
    }
}
