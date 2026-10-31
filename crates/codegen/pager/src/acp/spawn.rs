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
        ConfigUpdate::SubagentToggleChanged => (
            "grow/internal/reload_subagent_toggle",
            serde_json::json!({}),
        ),
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

async fn apply_config_update(
    agent: &MvpAgent,
    update: shell::config::reloader::ConfigUpdate,
) -> std::result::Result<(), acp::Error> {
    let Some(request) = config_reload_request(update) else {
        return Ok(());
    };
    shell::extensions::session_admin::handle(agent, &request)
        .await
        .map(|_| ())
}

enum GatewayRegistrationRace<R> {
    Gateway(std::result::Result<(), tokio::task::JoinError>),
    Registration(R),
}

async fn race_gateway_with_registration<R>(
    gateway: &mut tokio::task::JoinHandle<()>,
    registration: impl std::future::Future<Output = R>,
) -> GatewayRegistrationRace<R> {
    tokio::select! {
        biased;
        result = gateway => GatewayRegistrationRace::Gateway(result),
        result = registration => GatewayRegistrationRace::Registration(result),
    }
}

async fn cancel_gateway_and_flush(
    gateway: &mut tokio::task::JoinHandle<()>,
    agent: &MvpAgent,
    cancel: &CancellationToken,
    watcher_tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    cancel.cancel();
    gateway.abort();
    let _ = gateway.await;
    stop_watchers_and_flush(agent, cancel, watcher_tasks).await;
}

async fn stop_watchers_and_flush(
    agent: &MvpAgent,
    cancel: &CancellationToken,
    watcher_tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    cancel.cancel();
    for task in watcher_tasks.drain(..) {
        task.abort();
        let _ = task.await;
    }
    agent.flush_all_sessions(SESSION_FLUSH_GRACE).await;
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WatcherRegistrationKind {
    Config,
    Skills,
}

#[cfg(test)]
struct ConfigWatcherRegistrationGate {
    home: std::path::PathBuf,
    kind: WatcherRegistrationKind,
    started: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

#[cfg(test)]
static CONFIG_WATCHER_REGISTRATION_GATE: std::sync::OnceLock<
    std::sync::Mutex<Option<ConfigWatcherRegistrationGate>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
struct ConfigWatcherRegistrationGateGuard;

#[cfg(test)]
impl Drop for ConfigWatcherRegistrationGateGuard {
    fn drop(&mut self) {
        CONFIG_WATCHER_REGISTRATION_GATE
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .unwrap()
            .take();
    }
}

#[cfg(test)]
fn arm_config_watcher_registration_gate(
    home: std::path::PathBuf,
    kind: WatcherRegistrationKind,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
    ConfigWatcherRegistrationGateGuard,
) {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    *CONFIG_WATCHER_REGISTRATION_GATE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap() = Some(ConfigWatcherRegistrationGate {
        home,
        kind,
        started: started_tx,
        release: release_rx,
    });
    (started_rx, release_tx, ConfigWatcherRegistrationGateGuard)
}

#[cfg(test)]
fn block_watcher_registration_for_test(home: &std::path::Path, kind: WatcherRegistrationKind) {
    let gate = {
        let mut slot = CONFIG_WATCHER_REGISTRATION_GATE
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .unwrap();
        if slot
            .as_ref()
            .is_some_and(|gate| gate.home.as_path() == home && gate.kind == kind)
        {
            slot.take()
        } else {
            None
        }
    };
    if let Some(gate) = gate {
        let _ = gate.started.send(());
        let _ = gate.release.recv();
    }
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

                // Install path updates before the gateway can accept sessions.
                // They stay queued until blocking config watcher registration
                // completes and the LocalSet starts its consumer.
                let (config_update_tx, mut config_update_rx) =
                    tokio::sync::mpsc::unbounded_channel();
                let project_watch_enabled = shell::util::config::resolve_mcp_recursive_config_watch(
                    shell::config::load_from_disk().ok().as_ref(),
                );
                let cwd = std::env::current_dir().unwrap_or_default();
                let config_path_rx = if project_watch_enabled {
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    agent.set_config_watcher_path_tx(tx);
                    Some(rx)
                } else {
                    None
                };
                let agent_rc = Rc::new(agent);
                let gw_rx = AcpGatewayReceiver::new(channel.rx, agent_rc.clone()).with_tracing(true);
                let mut gateway_task = tokio::task::spawn_local(gw_rx.run());
                let mut watcher_tasks = Vec::new();

                let watcher_home = shell::util::grow_home::grow_home();
                let watcher_cancel = cancel.clone();
                let watcher_cwd = project_watch_enabled.then_some(cwd);
                let startup_config_update_tx = config_update_tx.clone();
                let config_registration = tokio::task::spawn_blocking(move || {
                    #[cfg(test)]
                    block_watcher_registration_for_test(
                        &watcher_home,
                        WatcherRegistrationKind::Config,
                    );
                    shell::config::reloader::start_config_reload(
                        &watcher_home,
                        &[],
                        watcher_cwd.as_deref(),
                        None,
                        config_update_tx,
                        experimental_memory,
                        no_memory,
                        watcher_cancel,
                    )
                });
                let config_result = race_gateway_with_registration(
                    &mut gateway_task,
                    shell::agent::app::finish_blocking_registration(
                        config_registration,
                        &cancel,
                    ),
                )
                .await;
                let config_watcher = match config_result {
                    GatewayRegistrationRace::Gateway(result) => {
                        stop_watchers_and_flush(&agent_rc, &cancel, &mut watcher_tasks).await;
                        result?;
                        return Ok(());
                    }
                    GatewayRegistrationRace::Registration(Ok(Some(watcher))) => watcher,
                    GatewayRegistrationRace::Registration(Ok(None)) => {
                        cancel_gateway_and_flush(
                            &mut gateway_task,
                            &agent_rc,
                            &cancel,
                            &mut watcher_tasks,
                        )
                        .await;
                        return Ok(());
                    }
                    GatewayRegistrationRace::Registration(Err(error)) => {
                        cancel_gateway_and_flush(
                            &mut gateway_task,
                            &agent_rc,
                            &cancel,
                            &mut watcher_tasks,
                        )
                        .await;
                        return Err(error.into());
                    }
                };
                let _config_watcher = if let Some(watcher) = config_watcher {
                    Some(Rc::new(std::cell::RefCell::new(watcher)))
                } else {
                    None
                };

                // Start config processing as soon as its watcher is ready.
                // Skills watcher registration is an independent blocking OS
                // call and must not pause MCP catalog or policy hot reloads.
                if let (Some(watcher), Some(mut path_rx)) =
                    (_config_watcher.as_ref(), config_path_rx)
                {
                    while let Ok(update) = path_rx.try_recv() {
                        watcher.borrow_mut().apply_session_update(update);
                    }
                    let watcher_for_paths = watcher.clone();
                    let cancel_paths = cancel.clone();
                    watcher_tasks.push(tokio::task::spawn_local(async move {
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
                    }));
                }

                let agent_for_updates = agent_rc.clone();
                let cancel_updates = cancel.clone();
                watcher_tasks.push(tokio::task::spawn_local(async move {
                    loop {
                        let update = tokio::select! {
                            biased;
                            _ = cancel_updates.cancelled() => break,
                            update = config_update_rx.recv() => match update {
                                Some(update) => update,
                                None => break,
                            },
                        };
                        if let Err(error) = apply_config_update(&agent_for_updates, update).await {
                            tracing::warn!(error = ?error, "failed to apply in-process config reload");
                        }
                    }
                }));
                let _ = startup_config_update_tx.send(
                    shell::config::reloader::ConfigUpdate::SubagentToggleChanged,
                );
                let _ = startup_config_update_tx.send(
                    shell::config::reloader::ConfigUpdate::McpCatalogChanged {
                        project_root: None,
                    },
                );

                #[cfg(test)]
                let skills_home = shell::util::grow_home::grow_home();
                let skills_registration = tokio::task::spawn_blocking(move || {
                    #[cfg(test)]
                    block_watcher_registration_for_test(
                        &skills_home,
                        WatcherRegistrationKind::Skills,
                    );
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let workspace_user_dir =
                        agent::prompt::workspace_user::optional_workspace_user_dir();
                    shell::config::watcher::SkillsFileWatcher::start(
                        Some(cwd.as_path()),
                        workspace_user_dir.as_deref(),
                        &skills_paths,
                    )
                });
                let skills_result = race_gateway_with_registration(
                    &mut gateway_task,
                    shell::agent::app::finish_blocking_registration(skills_registration, &cancel),
                )
                .await;
                let skills_watcher = match skills_result {
                    GatewayRegistrationRace::Gateway(result) => {
                        stop_watchers_and_flush(&agent_rc, &cancel, &mut watcher_tasks).await;
                        result?;
                        return Ok(());
                    }
                    GatewayRegistrationRace::Registration(Ok(Some(watcher))) => watcher,
                    GatewayRegistrationRace::Registration(Ok(None)) => {
                        cancel_gateway_and_flush(
                            &mut gateway_task,
                            &agent_rc,
                            &cancel,
                            &mut watcher_tasks,
                        )
                        .await;
                        return Ok(());
                    }
                    GatewayRegistrationRace::Registration(Err(error)) => {
                        cancel_gateway_and_flush(
                            &mut gateway_task,
                            &agent_rc,
                            &cancel,
                            &mut watcher_tasks,
                        )
                        .await;
                        return Err(error.into());
                    }
                };
                if let Some((mut watcher, mut skills_rx)) = skills_watcher {
                    let agent = agent_rc.clone();
                    let cancel_skills = cancel.clone();
                    watcher_tasks.push(tokio::task::spawn_local(async move {
                        loop {
                            let change = tokio::select! {
                                biased;
                                _ = cancel_skills.cancelled() => break,
                                change = skills_rx.recv() => match change {
                                    Some(change) => change,
                                    None => break,
                                },
                            };
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
                    }));
                }

                // Keep running until cancelled, then flush every live session
                // actor (SessionEnd hooks + memory save) before the LocalSet /
                // agent drop. Session actors live on dedicated OS threads and
                // only exit cleanly on SessionCommand::Shutdown; without this
                // flush, /quit and headless quit race process death and skip
                // SessionEnd. Mirrors leader auto-update / relaunch.
                let gateway_result = tokio::select! {
                    biased;
                    result = &mut gateway_task => Some(result),
                    _ = cancel.cancelled() => None,
                };
                cancel.cancel();
                if gateway_result.is_none() {
                    gateway_task.abort();
                    let _ = gateway_task.await;
                }
                stop_watchers_and_flush(&agent_rc, &cancel, &mut watcher_tasks).await;
                if let Some(result) = gateway_result {
                    result?;
                }
                anyhow::Result::Ok(())
            })
        })?)
}

#[cfg(test)]
mod tests {
    mod mcp_hot_reload;

    #[test]
    fn subagent_toggle_reload_uses_shared_internal_route() {
        let request = super::config_reload_request(
            shell::config::reloader::ConfigUpdate::SubagentToggleChanged,
        )
        .unwrap();
        assert_eq!(
            request.method.as_ref(),
            "grow/internal/reload_subagent_toggle"
        );
        assert_eq!(request.params.get(), "{}");
    }
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

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_completion_preempts_blocked_watcher_registration() {
        struct RegistrationOwner(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for RegistrationOwner {
            fn drop(&mut self) {
                if let Some(dropped_tx) = self.0.take() {
                    let _ = dropped_tx.send(());
                }
            }
        }

        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_started_tx, gateway_started_rx) = tokio::sync::oneshot::channel();
                let (gateway_finished_tx, gateway_finished_rx) = tokio::sync::oneshot::channel();
                let mut gateway = tokio::task::spawn_local(async move {
                    let _ = gateway_started_tx.send(());
                    gateway_finished_rx.await.unwrap();
                });
                gateway_started_rx.await.unwrap();

                let (registration_started_tx, registration_started_rx) =
                    tokio::sync::oneshot::channel();
                let (registration_release_tx, registration_release_rx) = std::sync::mpsc::channel();
                let (owner_dropped_tx, owner_dropped_rx) = tokio::sync::oneshot::channel();
                let registration = tokio::task::spawn_blocking(move || {
                    let _ = registration_started_tx.send(());
                    registration_release_rx.recv().unwrap();
                    RegistrationOwner(Some(owner_dropped_tx))
                });
                registration_started_rx.await.unwrap();

                gateway_finished_tx.send(()).unwrap();

                let cancel = CancellationToken::new();
                let outcome = race_gateway_with_registration(
                    &mut gateway,
                    shell::agent::app::finish_blocking_registration(registration, &cancel),
                )
                .await;
                assert!(matches!(outcome, GatewayRegistrationRace::Gateway(Ok(()))));

                cancel.cancel();
                registration_release_tx.send(()).unwrap();
                owner_dropped_rx
                    .await
                    .expect("the blocking registration task must drop its eventual watcher");
            })
            .await;
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
