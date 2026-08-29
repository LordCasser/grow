//! The session actor's main loop (`run_session`): command dispatch, idle
//! arms, and the free helpers only the loop consumes.
#![allow(clippy::items_after_test_module)]
use super::*;
use futures_util::FutureExt as _;

/// Returns the authoritative mode only when the manager accepted a real
/// transition. Callers pass the post-clamp read-back, never the request.
pub(super) fn permission_mode_change(
    was: ::diagnostics::enums::PermissionMode,
    actual: ::diagnostics::enums::PermissionMode,
) -> Option<::diagnostics::enums::PermissionMode> {
    (was != actual).then_some(actual)
}

async fn join_control_worker(
    label: &str,
    mut handle: Option<tokio::task::JoinHandle<Result<(), String>>>,
) -> Result<(), String> {
    let Some(handle) = handle.as_mut() else {
        return Ok(());
    };
    match tokio::time::timeout(std::time::Duration::from_secs(5), &mut *handle).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(error)) => Err(format!("{label} control worker join failed: {error}")),
        Err(_) => {
            handle.abort();
            let _ = handle.await;
            Err(format!("{label} control worker did not stop within 5s"))
        }
    }
}

async fn quiesce_control_workers(session: &SessionActor) -> Result<(), String> {
    let mut step = session.step_control_worker.take();
    let mut behavior = session.behavior_control_worker.take();
    let (step_result, behavior_result) = tokio::join!(
        join_control_worker("Sampling/Agent", step.take()),
        join_control_worker("Behavior", behavior.take()),
    );
    let mut errors = Vec::new();
    if let Err(error) = step_result {
        errors.push(error);
    }
    if let Err(error) = behavior_result {
        errors.push(error);
    }
    let state = session.state.lock().await;
    if matches!(state.foreground, ForegroundState::ApplyingControl)
        || state.behavior_control_worker_active
        || state.applying_step_control.is_some()
        || state.applying_behavior_control.is_some()
    {
        errors.push("control worker stopped without releasing actor ownership".to_string());
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(())
}

struct GracefulShutdown {
    reason: &'static str,
}

pub(super) async fn latch_termination_and_cancel_controls(
    session: &SessionActor,
    requested: TerminationState,
) -> Result<(), String> {
    let _gate = session.step_control_gate.lock().await;
    let pending_behavior = {
        let mut state = session.state.lock().await;
        state.termination.request(requested);
        if matches!(requested, TerminationState::Graceful)
            && matches!(state.termination, TerminationState::Fatal)
        {
            return Err("session already crossed a fatal persistence boundary".to_string());
        }
        state.pending_step_controls.cancel_for_shutdown();
        state.pending_behavior_control.take()
    };
    if let Some(pending) = pending_behavior {
        let _ = pending.responds_to.send(Err(
            acp::Error::internal_error().data("session is shutting down")
        ));
    }
    // Workflow is another Session-owned admission surface. Latch it in the
    // same termination transition so a fatal/graceful teardown cannot leave
    // its detached ingress accepting a queued envelope while other owners
    // are being drained.
    session.workflow_manager.lock().await.close_admission();
    session.workflow_service_shutdown.cancel();
    session.idle_arbiter.notify_waiters();
    Ok(())
}

/// Cancel every queued owner except the exact foreground turn still settling.
/// Shutdown cannot drop a `session/prompt` responder, and it cannot use FIFO
/// position as a proxy for foreground ownership.
async fn reject_queued_inputs_for_shutdown(session: &SessionActor) {
    let removed = {
        let mut state = session.state.lock().await;
        state.pending_manual_compact = None;
        let running = state.running_prompt_id().map(str::to_owned);
        let mut kept = std::collections::VecDeque::new();
        let mut removed = Vec::new();
        for input in std::mem::take(&mut state.pending_inputs) {
            if running.as_deref() == Some(input.prompt_id.as_str()) {
                kept.push_back(input);
            } else {
                removed.push(input);
            }
        }
        state.pending_inputs = kept;
        session.broadcast_queue_changed(&state);
        removed
    };
    for input in removed {
        SessionActor::respond_removed_prompt(input.respond_to);
    }
}

/// Cancel every non-Workflow Task child and keep the root Goal accounting
/// ingress alive until those child SessionActors have reached their terminal.
/// The public Session is already termination-latched, so unrelated commands
/// are rejected by dropping their responders; only immutable usage facts are
/// allowed to cross this drain boundary.
pub(super) async fn cancel_and_drain_session_subagents(
    session: &SessionActor,
    cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
) -> Result<(), String> {
    let Some(event_tx) = session.tool_context.subagent_event_tx.clone() else {
        return Ok(());
    };
    use tools::implementations::grow_build::task::backend::ChannelBackend;
    use tools::implementations::grow_build::task::types::SubagentCancelOutcome;
    let backend = ChannelBackend::for_session(event_tx, session.session_id_string());
    let (respond_to, mut drained) = tokio::sync::oneshot::channel();
    if !backend.request_cancel_parent_session(respond_to) {
        return Ok(());
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            tokio::select! {
                outcome = &mut drained => {
                    return match outcome.unwrap_or(SubagentCancelOutcome::NotFound) {
                        SubagentCancelOutcome::Cancelled
                        | SubagentCancelOutcome::AlreadyFinished { .. }
                        | SubagentCancelOutcome::NotFound => Ok(()),
                    };
                }
                command = cmd_rx.recv() => {
                    let Some(command) = command else {
                        let _ = (&mut drained).await;
                        return Ok(());
                    };
                    let _control = session.step_control_gate.lock().await;
                    let result = match command {
                        SessionCommand::RecordGoalUsage { goal_id, tokens, respond_to } => {
                            let result = session.apply_captured_goal_usage(&goal_id, tokens).await;
                            let _ = respond_to.send(result.clone());
                            result.map(|_| ())
                        }
                        SessionCommand::RecordGoalUsageIncomplete { goal_id, respond_to } => {
                            let result = session.apply_captured_goal_usage_incomplete(&goal_id).await;
                            let _ = respond_to.send(result.clone());
                            result.map(|_| ())
                        }
                        SessionCommand::SettleGoalUsageAttempt { attempt_id, respond_to } => {
                            let result = session.settle_claimed_goal_usage_attempt(&attempt_id).await;
                            let _ = respond_to.send(result.clone());
                            result.map(|_| ())
                        }
                        _ => Ok(()),
                    };
                    result?;
                }
            }
        }
    })
    .await
    .map_err(|_| "timed out draining non-Workflow subagent sessions".to_string())?
}

async fn begin_graceful_shutdown(
    session: &std::sync::Arc<SessionActor>,
    replay_buffer: &mut ReplayBuffer,
    cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
) -> Result<(), String> {
    session.session_activities.close_admission();
    session.sideband_cancel.cancel();
    session
        .user_input_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    stop_session_background_services(session, false)
        .await
        .map_err(|error| format!("background service shutdown failed: {error}"))?;
    session
        .goal_drive
        .abort_and_join()
        .await
        .map_err(|error| format!("Goal drive shutdown failed: {error}"))?;
    session
        .deferred_prefix
        .abort_and_join()
        .await
        .map_err(|error| format!("prefix preparation shutdown failed: {error}"))?;
    session
        .restored_plan_approval
        .abort_and_join()
        .await
        .map_err(|error| format!("restored Plan approval shutdown failed: {error}"))?;
    quiesce_control_workers(session).await?;
    if let Some(notification) = replay_buffer.flush() {
        session.emit_buffered(notification).await;
    }
    reject_queued_inputs_for_shutdown(session).await;
    session
        .cancel_running_task(true, true, false, Some("shutdown".to_owned()))
        .await
        .map_err(|error| format!("shutdown foreground cancellation failed: {error:?}"))?;
    shutdown_sampler(session).await?;
    cancel_and_drain_session_subagents(session, cmd_rx).await?;
    cmd_rx.close();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        session.session_activities.wait_idle(),
    )
    .await
    .map_err(|_| "timed out draining session auxiliary work".to_string())?;
    // A config command admitted immediately before the latch may have
    // completed MCP initialization after the first service stop. Revoke the
    // resulting ingress once more after every finite command owner has joined.
    stop_session_background_services(session, false)
        .await
        .map_err(|error| {
            format!("background service shutdown failed after owner drain: {error}")
        })?;
    Ok(())
}

async fn graceful_shutdown_ready(session: &SessionActor) -> bool {
    let state = session.state.lock().await;
    matches!(state.termination, TerminationState::Graceful)
        && state.foreground.is_idle()
        && state.pending_inputs.is_empty()
        && state.pending_manual_compact.is_none()
        && state.pending_step_controls.is_empty()
        && state.pending_behavior_control.is_none()
        && state.applying_step_control.is_none()
        && state.applying_behavior_control.is_none()
        && !state.behavior_control_worker_active
        && session.session_activities.is_idle()
        && !session
            .memory
            .is_flushing
            .load(std::sync::atomic::Ordering::Acquire)
        && !session
            .memory
            .is_dreaming
            .load(std::sync::atomic::Ordering::Acquire)
}

async fn finish_graceful_shutdown(
    session: &SessionActor,
    replay_buffer: &mut ReplayBuffer,
    cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
    shutdown: GracefulShutdown,
) {
    let permission_audit_error = stop_permission_manager_and_drain_audit(session).await.err();
    if let Err(run_ids) = shutdown_workflows(session).await {
        tracing::error!(
            ?run_ids,
            "Workflow owners did not drain; withholding Goal close and final persistence frontier"
        );
        session.fail_stop_sideband_admission().await;
        session.sideband_cancel.cancel();
        session.finalizer_sideband_cancel.cancel();
        session.sideband_repair_cancel.cancel();
        session.durable_ui_cancel.cancel();
        session.signals_handle.shutdown();
        cleanup_session_scratch(session);
        return;
    }
    if let Some(notification) = replay_buffer.flush() {
        session.emit_buffered(notification).await;
    }
    session.drop_pending_synthetic_items().await;

    // SessionEnd is admitted only after every foreground/control owner has
    // reached its terminal. Hooks and memory are therefore the final normal
    // producers before the persistence barrier.
    let envelope = session.fire_hook(
        ::hooks::event::HookEventName::SessionEnd,
        None,
        ::hooks::event::HookPayload::SessionEnd {
            reason: shutdown.reason.to_string(),
            turn_count: None,
            tool_call_count: None,
        },
    );
    if let Some(registry) = session.hooks.registry.borrow().clone() {
        let ctx = session.hook_run_ctx();
        let results = ::hooks::dispatcher::dispatch_non_blocking(
            &registry,
            ::hooks::event::HookEventName::SessionEnd,
            &envelope,
            &ctx,
        )
        .await;
        session
            .send_hook_execution("session_end", None, None, &results)
            .await;
    }
    session.dispatch_session_end_stop(shutdown.reason).await;

    let mut session_end_result = "disabled";
    let mut total_chunks_at_end = 0usize;
    if !session.startup_hints.is_subagent {
        if let Some(storage) = session.memory.storage() {
            let conversation = session.chat_state_handle.get_conversation().await;
            let result = crate::session::memory::hooks::on_session_end(
                &storage,
                &conversation,
                &session.session_info.id.0,
                session.memory.save_on_end,
            );
            session_end_result = match &result {
                crate::session::memory::hooks::SessionEndResult::Written(_) => "written",
                crate::session::memory::hooks::SessionEndResult::Skipped => "skipped",
                crate::session::memory::hooks::SessionEndResult::Failed(_) => "failed",
            };
            total_chunks_at_end = storage.total_chunk_count();
            let telem = session.memory.diagnostics_snapshot();
            tracing::info!(
                target: ::diagnostics::memory_log::TARGET,
                reason = shutdown.reason,
                result = ?result,
                tool_searches = telem.tool_search_count,
                injection_searches = telem.injection_count,
                recovery_searches = telem.compaction_recovery_count,
                "MEMORY_SESSION_END: session summary saved"
            );
            if let crate::session::memory::hooks::SessionEndResult::Written(ref path_str) = result {
                session
                    .reindex_and_embed(std::path::Path::new(path_str), "session")
                    .await;
                session
                    .send_grow_notification(GrowSessionUpdate::MemorySessionSaved {
                        path: path_str.clone(),
                    })
                    .await;
            }
        }
    } else {
        tracing::debug!(
            target: ::diagnostics::memory_log::TARGET,
            "MEMORY_SUBAGENT_SKIP: skipping on_session_end for subagent session"
        );
    }
    if tokio::time::timeout(
        std::time::Duration::from_secs(30),
        session.maybe_run_dream(true),
    )
    .await
    .is_err()
    {
        tracing::warn!("SessionEnd memory dream timed out; revoking finalizer Sideband work");
        session.finalizer_sideband_cancel.cancel();
        session.sideband_repair_cancel.cancel();
    }
    // A Sideband owner can transfer its terminal lease to Drop recovery. Wait
    // for that exact nested owner before closing Goal accounting or crossing
    // the final persistence barrier.
    if tokio::time::timeout(
        std::time::Duration::from_secs(5),
        session.session_activities.wait_idle(),
    )
    .await
    .is_err()
    {
        tracing::warn!("SessionEnd auxiliary drain timed out; revoking Sideband persistence epoch");
        session.fail_stop_sideband_admission().await;
        session.finalizer_sideband_cancel.cancel();
        session.sideband_repair_cancel.cancel();
        if tokio::time::timeout(
            std::time::Duration::from_secs(5),
            session.session_activities.wait_idle(),
        )
        .await
        .is_err()
        {
            tracing::error!(
                "SessionEnd auxiliary owners remained live after revocation; withholding Goal close and final persistence frontier"
            );
            session.sideband_cancel.cancel();
            session.durable_ui_cancel.cancel();
            session.signals_handle.shutdown();
            cleanup_session_scratch(session);
            return;
        }
    }
    let telem = session.memory.diagnostics_snapshot();
    session.emit_memory_session_summary(&telem, total_chunks_at_end, session_end_result);
    if let Some(signals) = session.signals_handle().snapshot().await {
        ::diagnostics::session_ctx::log_event(::diagnostics::events::SessionEnded {
            duration_secs: session.session_start.elapsed().as_secs(),
            turn_count: signals.turn_count as u64,
            tool_call_count: signals.tool_call_count as u64,
            compaction_count: signals.compaction_count as u64,
            model_id: session.current_catalog_model_id(),
        });
    }
    if !session.startup_hints.is_subagent {
        session.checkpoint_running_task_notifications().await;
    }
    // Goal usage covers every provider call admitted while the Goal was
    // active, including Workflow children and the session-end memory/dream
    // producers above. Close and claim that window only after every such
    // owner has drained, immediately before the final persistence barrier.
    if let Err(error) = session.settle_goal_usage_for_shutdown().await {
        tracing::error!(%error, "failed to settle Goal usage at shutdown");
        terminate_failed_timeline_writer(session, cmd_rx).await;
        return;
    }
    session.checkpoint_goal_before_shutdown().await;
    if let Some(error) = permission_audit_error {
        tracing::error!(
            %error,
            "permission audit did not reach durability; withholding final persistence frontier"
        );
        session.fail_stop_sideband_admission().await;
        session.sideband_cancel.cancel();
        session.finalizer_sideband_cancel.cancel();
        session.sideband_repair_cancel.cancel();
        session.durable_ui_cancel.cancel();
        session.signals_handle.shutdown();
        cleanup_session_scratch(session);
        return;
    }
    final_session_persistence_flush(session).await;
    session.signals_handle.shutdown();
    cleanup_session_scratch(session);
}
#[cfg(test)]
mod permission_mode_change_tests {
    use super::permission_mode_change;
    use ::diagnostics::enums::PermissionMode;

    #[test]
    fn reports_actual_state_change_only() {
        assert_eq!(
            permission_mode_change(PermissionMode::Ask, PermissionMode::Ask),
            None
        );
        assert_eq!(
            permission_mode_change(PermissionMode::Ask, PermissionMode::Auto),
            Some(PermissionMode::Auto)
        );
        assert_eq!(
            permission_mode_change(PermissionMode::Auto, PermissionMode::AlwaysApprove),
            Some(PermissionMode::AlwaysApprove)
        );
    }
}

#[cfg(test)]
mod shutdown_queue_tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_rejection_keeps_the_settling_owner_and_resolves_queued_prompts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let (settling, mut settling_rx) =
                    crate::session::actor::tests::support::user_item_with_rx(
                        "settling-owner",
                        "test-client",
                    );
                let (queued, queued_rx) = crate::session::actor::tests::support::user_item_with_rx(
                    "queued-during-settlement",
                    "test-client",
                );

                {
                    let mut state = actor.state.lock().await;
                    state.foreground = ForegroundState::Settling {
                        prompt_id: "settling-owner".into(),
                        origin: crate::session::PromptOrigin::User,
                        turn_kind: crate::session::TurnKind::User,
                    };
                    state.pending_inputs.push_back(settling);
                    state.pending_inputs.push_back(queued);
                }

                reject_queued_inputs_for_shutdown(&actor).await;

                let state = actor.state.lock().await;
                assert_eq!(state.pending_inputs.len(), 1);
                assert_eq!(
                    state
                        .pending_inputs
                        .front()
                        .map(|item| item.prompt_id.as_str()),
                    Some("settling-owner")
                );
                drop(state);

                let result = queued_rx
                    .await
                    .expect("shutdown must resolve queued prompt RPC");
                let result = result.expect("removed queued prompt is not a turn failure");
                assert!(matches!(
                    result.completion_kind,
                    crate::session::commands::PromptCompletionKind::RemovedFromQueue
                ));
                assert!(
                    matches!(
                        settling_rx.try_recv(),
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                    ),
                    "the exact settling owner must remain unresolved for its terminal owner"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn shutdown_rejection_removes_all_queued_prompts_when_compaction_owns_foreground() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let (first, first_rx) = crate::session::actor::tests::support::user_item_with_rx(
                    "queued-before-compaction",
                    "test-client",
                );
                let (second, second_rx) = crate::session::actor::tests::support::user_item_with_rx(
                    "queued-after-compaction",
                    "test-client",
                );
                {
                    let mut state = actor.state.lock().await;
                    state.foreground = ForegroundState::Compaction;
                    state.pending_inputs.push_back(first);
                    state.pending_inputs.push_back(second);
                }

                reject_queued_inputs_for_shutdown(&actor).await;

                assert!(actor.state.lock().await.pending_inputs.is_empty());
                for receiver in [first_rx, second_rx] {
                    let result = receiver
                        .await
                        .expect("shutdown must resolve every queued prompt RPC")
                        .expect("removed queued prompt is not a turn failure");
                    assert!(matches!(
                        result.completion_kind,
                        crate::session::commands::PromptCompletionKind::RemovedFromQueue
                    ));
                }
            })
            .await;
    }
}

pub(super) async fn run_session(
    session: Arc<SessionActor>,
    mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    mut chat_state_event_rx: mpsc::UnboundedReceiver<chat_state::ChatStateEvent>,
    mut event_rx: mpsc::UnboundedReceiver<SessionEvent>,
    fs_notify_config: Option<ClientFsConfig>,
    codebase_indexes: std::sync::Arc<parking_lot::Mutex<CodebaseIndexManager>>,
    index_root: std::path::PathBuf,
    fs_watch_caps: fs_watch::FsWatchCapabilities,
) {
    let (completion_tx, mut completion_rx) =
        mpsc::unbounded_channel::<(String, PromptTurnResult)>();
    tracing::debug!("fs_notify_config: {:?}", fs_notify_config);
    let mut replay_buffer = ReplayBuffer::new(session.buffering_settings.clone());
    let event_tx_for_flush_timer = session.event_tx.clone();
    let buffering_flush_interval = replay_buffer.max_wait_duration_ms();
    if let Some(buffering_flush_interval) = buffering_flush_interval {
        tokio::task::spawn_local(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(std::cmp::max(
                20,
                buffering_flush_interval * 2,
            )));
            loop {
                interval.tick().await;
                if event_tx_for_flush_timer
                    .send(SessionEvent::FlushReplay { respond_to: None })
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    if let Some((mut watcher, mut changes)) = crate::config::watcher::ProjectDiscoveryWatcher::start(
        std::path::Path::new(session.session_info.cwd.as_str()),
    ) {
        let watch_session = session.clone();
        let handle = tokio::task::spawn_local(async move {
            while let Some(change) = changes.recv().await {
                watcher.refresh_new_dirs();
                match change {
                    crate::config::watcher::DiscoveryChange::Skills => {
                        watch_session.reload_skills_from_disk().await;
                    }
                    crate::config::watcher::DiscoveryChange::Workflows => {
                        watch_session.send_available_commands_update().await;
                    }
                }
            }
        });
        session.project_discovery_worker.arm(handle);
    }
    let fs_watch_handle = if fs_watch_caps.needs_watcher() {
        let deps = fs_watch::FsWatchDeps::from_session(
            &session,
            fs_notify_config.clone(),
            codebase_indexes.clone(),
            index_root.clone(),
        );
        tracing::debug!(?fs_watch_caps, "fs-notify: spawning");
        Some(fs_watch::spawn(fs_watch::FsWatchPlan::build(
            fs_watch_caps,
            deps,
        )))
    } else {
        tracing::debug!("fs-notify: skipped (no consumers)");
        None
    };
    session.fs_watch_handle.replace(fs_watch_handle);
    {
        let s = session.clone();
        if let Some(activity) = session.session_activities.try_start("git_branch_notice") {
            tokio::task::spawn_local(async move {
                let _activity = activity;
                s.maybe_notify_git_branch().await;
            });
        }
    }
    let liveness_watchers_enabled = {
        let user_cfg = crate::config::load_effective_config().ok();
        let requirements = crate::agent::config::read_requirements_toml();
        crate::util::config::resolve_mcp_liveness_watchers(
            requirements.as_ref(),
            user_cfg.as_ref(),
            None,
        )
    };
    if !session.startup_hints.is_subagent && liveness_watchers_enabled {
        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<::mcp::servers::McpClientEvent>();
        {
            let mut mcp_state = session.mcp_state.lock().await;
            mcp_state.set_client_event_tx(Some(event_tx));
        }
        let dispatcher_session_id = session.session_info.id.0.to_string();
        let dispatcher_cwd = std::path::PathBuf::from(session.session_info.cwd.as_str());
        let dispatcher_gateway = session.notifications.gateway.clone();
        let dispatcher_mcp_state = Arc::clone(&session.mcp_state);
        let shutdown_state = crate::session::mcp_dispatcher::new_shutdown_state();
        let auto_restart_enabled = {
            let user_cfg = crate::config::load_effective_config().ok();
            let requirements = crate::agent::config::read_requirements_toml();
            crate::util::config::resolve_mcp_auto_restart(
                requirements.as_ref(),
                user_cfg.as_ref(),
                None,
            )
        };
        let restart_actions: Option<std::rc::Rc<dyn crate::session::mcp_restart::RestartActions>> =
            if auto_restart_enabled {
                Some(std::rc::Rc::new(SessionRestartActions::new(
                    session.clone(),
                    Arc::clone(&shutdown_state),
                )))
            } else {
                None
            };
        let handle = tokio::task::spawn_local(async move {
            crate::session::mcp_dispatcher::run_dispatcher(
                dispatcher_session_id,
                event_rx,
                dispatcher_gateway,
                dispatcher_mcp_state,
                shutdown_state,
                restart_actions,
                dispatcher_cwd,
            )
            .await;
        });
        session.mcp_dispatcher_worker.arm(handle);
    }
    let session_for_mcp = session.clone();
    let completion_tx_for_mcp = completion_tx.clone();
    if let Some(activity) = session.session_activities.try_start("mcp_initialization") {
        tokio::task::spawn_local(async move {
            let _activity = activity;
            session_for_mcp.ensure_mcp_tools_initialized().await;
            SessionActor::maybe_start_running_task(
                session_for_mcp.clone(),
                completion_tx_for_mcp.clone(),
            )
            .await;
            SessionActor::maybe_drain_notifications(session_for_mcp, completion_tx_for_mcp).await;
        });
    }
    let mut model_switch_rx = session.models_manager.subscribe_model_switch();
    let _ = *model_switch_rx.borrow_and_update();
    let idle_flush_sleep = match session.idle_flush_timeout {
        Some(timeout) => tokio::time::sleep(timeout),
        None => tokio::time::sleep(std::time::Duration::MAX),
    };
    tokio::pin!(idle_flush_sleep);
    let dream_check_sleep = match session.dream_check_timeout {
        Some(timeout) => tokio::time::sleep(timeout),
        None => tokio::time::sleep(std::time::Duration::MAX),
    };
    tokio::pin!(dream_check_sleep);
    let mut pending_shutdown: Option<GracefulShutdown> = None;
    let shutdown = loop {
        if pending_shutdown.is_some() && graceful_shutdown_ready(&session).await {
            break pending_shutdown
                .take()
                .expect("checked graceful shutdown request");
        }
        tokio::select! {
            biased;
            _ = session.session_activities.changed(), if pending_shutdown.is_some() => {
                // A finite detached owner reached its terminal. The readiness
                // predicate at the top of the loop decides whether this was
                // the final owner; no new work is admitted during shutdown.
            }
            // Idle flush timer fired — run background flush.
            _ = &mut idle_flush_sleep, if session.idle_flush_timeout.is_some()
                && pending_shutdown.is_none()
                && session.memory.is_enabled()
                && !session.memory.is_flushing.load(std::sync::atomic::Ordering::Relaxed) => {
                // Skip if no new messages since last idle flush
                let current_len = session.chat_state_handle.get_conversation_len().await;
                let last_len = session.last_idle_flush_conversation_len
                    .load(std::sync::atomic::Ordering::Relaxed);
                if current_len > last_len {
                    tracing::info!(target: ::diagnostics::memory_log::TARGET,
                        "MEMORY_IDLE_FLUSH: timer fired (conversation {last_len} → {current_len})");
                    session.last_idle_flush_conversation_len
                        .store(current_len, std::sync::atomic::Ordering::Relaxed);
                    let activity = session
                        .session_activities
                        .try_start("memory_idle_flush")
                        .expect("memory timer cannot fire after activity admission closes");
                    tokio::task::spawn_local({
                        let session = session.clone();
                        async move {
                            let _activity = activity;
                            if !session.run_memory_flush("interval", None).await {
                                tracing::info!(target: ::diagnostics::memory_log::TARGET,
                                    "MEMORY_IDLE_FLUSH: skipped — another flush already in progress");
                            }
                            session.idle_arbiter.notify_one();
                        }
                    });
                } else {
                    tracing::debug!(target: ::diagnostics::memory_log::TARGET,
                        "MEMORY_IDLE_FLUSH: skipped, no new messages since last flush (len={current_len})");
                }
                // Reset for next idle period
                if let Some(timeout) = session.idle_flush_timeout {
                    idle_flush_sleep.as_mut().reset(tokio::time::Instant::now() + timeout);
                }
            }
            // Dream check timer — periodically run dream consolidation.
            _ = &mut dream_check_sleep, if session.dream_check_timeout.is_some()
                && pending_shutdown.is_none()
                && session.memory.is_enabled() => {
                tracing::debug!(target: ::diagnostics::memory_log::TARGET,
                    "MEMORY_DREAM_CHECK: timer fired");
                let activity = session
                    .session_activities
                    .try_start("memory_dream_check")
                    .expect("dream timer cannot fire after activity admission closes");
                tokio::task::spawn_local({
                    let session = session.clone();
                    async move {
                        let _activity = activity;
                        session.maybe_run_dream(false).await;
                        session.idle_arbiter.notify_one();
                    }
                });
                if let Some(timeout) = session.dream_check_timeout {
                    dream_check_sleep.as_mut().reset(tokio::time::Instant::now() + timeout);
                }
            }
            // Layer-3 LazinessDetector: zero the per-session nudge
            // counter whenever the user switches models. The cap
            // is per-(session, model) — switching is a deliberate
            // user action that resets expectations. `.changed()`
            // only resolves on switches after the stream starts, so
            // there is no stored-permit hazard.
            changed = model_switch_rx.changed() => {
                if changed.is_ok() {
                    let new_gen = *model_switch_rx.borrow_and_update();
                    session.handle_model_switch_for_laziness(new_gen).await;
                }
            }
            // ChatStateActor events — coordination signals for session-level concerns.
            event = chat_state_event_rx.recv() => {
                match event {
                    Some(chat_state::ChatStateEvent::ConversationReset { new_len }) => {
                        // Reset idle-flush counter so next idle period flushes the new state.
                        session.last_idle_flush_conversation_len
                            .store(new_len, std::sync::atomic::Ordering::Relaxed);
                        // Re-arm the first-turn injection check after
                        // compaction (re-search only if no block persisted).
                        session.memory.context_injected
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                    Some(chat_state::ChatStateEvent::ImageBudget {
                        body_bytes,
                        trigger_bytes,
                        reclaim_target_bytes,
                        inline_images,
                        needs_image_compaction,
                        evicted,
                        body_bytes_after,
                    }) => {
                        // Unified-log record for local image-eviction verification.
                        ::diagnostics::unified_log::info(
                            "shell.image_budget",
                            Some(session.session_info.id.0.as_ref()),
                            Some(serde_json::json!({
                                "body_bytes": body_bytes,
                                "body_bytes_after": body_bytes_after,
                                "trigger_bytes": trigger_bytes,
                                "reclaim_target_bytes": reclaim_target_bytes,
                                "inline_images": inline_images,
                                "images_remaining": inline_images.saturating_sub(evicted),
                                "needs_image_compaction": needs_image_compaction,
                                "evicted": evicted,
                            })),
                        );
                    }
                    Some(chat_state::ChatStateEvent::PromptIndexChanged { .. }) => {
                        // Prompt-index updates are informational; consumers
                        // query the actor directly when they need them.
                    }
                    Some(chat_state::ChatStateEvent::ContextPressureUpdated {
                        projected_tokens,
                    }) => {
                        // Compression reads ChatState synchronously. This
                        // transient projection keeps both Pager render modes
                        // on the same fresh value without creating replay or
                        // model-context facts.
                        session.emit_context_pressure_update(projected_tokens);
                    }
                    None => {
                        tracing::error!(
                            "closing session because the Timeline writer actor stopped"
                        );
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                }
            }
            maybe_event = event_rx.recv() => {
                if let Some(event) = maybe_event {
                    match event {
                        SessionEvent::Notification(notification) => {
                            let out = replay_buffer.consume_chunk(notification);
                            match out {
                                None => {}
                                Some((first, second)) => {
                                    session.emit_buffered(first).await;
                                    if let Some(second) = second {
                                        session.emit_buffered(second).await;
                                    }
                                }
                            }
                        }
                        SessionEvent::ForegroundWake => {
                            let (terminating, regular) = {
                                let state = session.state.lock().await;
                                (!state.termination.is_open(), state.foreground.regular().is_some())
                            };
                            if terminating {
                                // Teardown continues to service flush and
                                // completion events, but never admits new work.
                            } else if regular {
                                session.drain_deferred_completions().await;
                            } else {
                                SessionActor::maybe_drain_notifications(
                                    session.clone(),
                                    completion_tx.clone(),
                                )
                                .await;
                            }
                        }
                        SessionEvent::ManualCompactionFinished { failure } => {
                            let (should_resume, fatal) = {
                                let mut state = session.state.lock().await;
                                if matches!(state.foreground, ForegroundState::Compaction) {
                                    state.foreground = ForegroundState::Idle;
                                } else {
                                    tracing::warn!(
                                        "manual compaction completion arrived without foreground ownership"
                                    );
                                }
                                if failure.is_some() {
                                    state.termination.request(TerminationState::Fatal);
                                }
                                (state.termination.is_open(), failure.is_some())
                            };
                            if fatal {
                                tracing::error!(
                                    message = failure.as_deref().unwrap_or("manual compaction failed"),
                                    "closing session after manual compaction owner failure"
                                );
                                cmd_rx.close();
                                terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                                return;
                            }
                            if should_resume {
                                arbitrate_idle_wake(session.clone(), completion_tx.clone()).await;
                                session.emit_session_idle_if_idle().await;
                            }
                        }
                        SessionEvent::FlushReplay { respond_to } => {
                            if let Some(notification) = replay_buffer.flush() {
                                session.emit_buffered(notification).await;
                            }

                            // Always ack (independent of whether anything was buffered).
                            if let Some(tx) = respond_to {
                                let _ = tx.send(());
                            }
                        }
                        SessionEvent::ControlWorkerFailed { message } => {
                            tracing::error!(%message, "closing session after control worker failure");
                            cmd_rx.close();
                            let _ = latch_termination_and_cancel_controls(
                                &session,
                                TerminationState::Fatal,
                            )
                            .await;
                            if let Err(error) = quiesce_control_workers(&session).await {
                                tracing::error!(%error, "failed to quiesce all control workers after fatal control failure");
                            }
                            if let Some(notification) = replay_buffer.flush() {
                                session.emit_buffered(notification).await;
                            }
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                    }
                }
            }
            maybe_completion = completion_rx.recv() => {
                let Some((prompt_id, result)) = maybe_completion else {
                    if !session.state.lock().await.foreground.is_idle() {
                        tracing::error!(
                            "completion channel closed while a foreground owner was still active"
                        );
                        let _ = latch_termination_and_cancel_controls(
                            &session,
                            TerminationState::Fatal,
                        )
                        .await;
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    if let Err(error) = latch_termination_and_cancel_controls(
                        &session,
                        TerminationState::Graceful,
                    )
                    .await
                    {
                        tracing::error!(%error, "failed to accept shutdown after completion channel closure");
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    if let Err(error) =
                        begin_graceful_shutdown(&session, &mut replay_buffer, &mut cmd_rx).await
                    {
                        tracing::error!(%error, "failed to begin shutdown after completion channel closure");
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    pending_shutdown = Some(GracefulShutdown {
                        reason: "completion_channel_closed",
                    });
                    continue;
                };
                // Flush any buffered turn deltas before `handle_completion`
                // emits the durable `TurnCompleted`, so the terminal lands
                // in updates.jsonl strictly after the turn's last
                // `session/update` delta. Mirrors the Cancel / Shutdown /
                // FlushComplete arms.
                if let Some(notification) = replay_buffer.flush() {
                    session.emit_buffered(notification).await;
                }
                // Capture ownership before `handle_completion` settles and
                // removes the foreground. Goal degradation is based on the
                // structured producer-stamped origin, never on the currently
                // selected Behavior: a user turn may fail while a background
                // Goal stage remains perfectly healthy.
                let completed_origin = {
                    let state = session.state.lock().await;
                    state.foreground.identity(&prompt_id).map(|(origin, _)| origin)
                };
                if result
                    .as_ref()
                    .err()
                    .is_some_and(crate::session::commands::is_fatal_turn_boundary_error)
                {
                    tracing::error!(
                        prompt_id,
                        "closing session after fatal Timeline turn-boundary failure"
                    );
                    session.handle_fatal_completion(prompt_id, result).await;
                    terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                    return;
                }
                let (_turn_succeeded, suppress_goal_continuation, goal_stop) =
                    SessionActor::post_turn_goal_degradation_plan(
                        &result,
                        completed_origin.as_ref(),
                    );
                session.handle_completion(prompt_id.clone(), result).await;
                if let Some(message) = goal_stop {
                    session.apply_goal_stop_after_turn(&prompt_id, message).await;
                }
                if pending_shutdown.is_some() {
                    // The producer epilogue and its exact prompt responder are
                    // now settled. Teardown owns the idle boundary, so queued
                    // work is rejected instead of being promoted.
                    reject_queued_inputs_for_shutdown(&session).await;
                    continue;
                }
                // Catalog watcher snapshots admitted during the turn must win
                // the newly released idle boundary before any queued prompt,
                // compaction, notification, or Goal continuation samples.
                session.apply_pending_step_controls_if_idle().await;
                if !maybe_start_pending_manual_compaction(session.clone()).await
                {
                    SessionActor::maybe_start_running_task(
                        session.clone(),
                        completion_tx.clone(),
                    )
                    .await;
                }
                // Fixed idle ordering: real user FIFO/manual compaction first,
                // then durable terminal/scheduler/progress receipts, then Goal.
                SessionActor::maybe_drain_notifications(
                    session.clone(),
                    completion_tx.clone(),
                )
                .await;
                if session.state.lock().await.foreground.is_idle() {
                    session
                        .handle_turn_end(&prompt_id, suppress_goal_continuation)
                        .await;
                }
                session.emit_session_idle_if_idle().await;
                // Layer-3 LazinessDetector: spawn an idle-triggered
                // classifier dispatch. The method is a no-op when the
                // per-model `laziness_detector.enabled = false`
                // (the v1 default for every model), so no
                // classification cost is incurred without explicit
                // opt-in. Spawned via `spawn_local` so the actor
                // loop can continue accepting commands while the
                // classifier idle-waits.
                {
                    let s = session.clone();
                    if let Some(activity) = session
                        .session_activities
                        .try_start("laziness_classifier")
                    {
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            s.maybe_fire_laziness_check().await;
                        });
                    }
                }
                if session.chat_state_handle.is_closed() {
                    tracing::error!(
                        "closing session because the Timeline writer mailbox is unavailable"
                    );
                    terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                    return;
                }
            }
            maybe_cmd = cmd_rx.recv(), if pending_shutdown.is_none() => {
                let Some(cmd) = maybe_cmd else {
                    if let Err(error) = latch_termination_and_cancel_controls(
                        &session,
                        TerminationState::Graceful,
                    )
                    .await
                    {
                        tracing::error!(%error, "failed to accept shutdown after command channel closure");
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    if let Err(error) =
                        begin_graceful_shutdown(&session, &mut replay_buffer, &mut cmd_rx).await
                    {
                        tracing::error!(%error, "failed to begin shutdown after command channel closure");
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    pending_shutdown = Some(GracefulShutdown {
                        reason: "channel_closed",
                    });
                    continue;
                };

                match cmd {
                    SessionCommand::SetGoalContextSnapshot { snapshot } => {
                        session
                            .agent
                            .borrow()
                            .tool_bridge()
                            .update_resource(
                                tools::implementations::grow_build::update_goal::GoalContextSnapshotResource(
                                    Some(snapshot),
                                ),
                            )
                            .await;
                    }
                    SessionCommand::RestorePlanApproval { respond_to } => {
                        // Reconcile missing/corrupt artifacts synchronously so
                        // load-session cannot acknowledge a wedged Plan. Only
                        // the open-ended user approval round-trip is detached.
                        let result = session.reconcile_restored_plan_approval().await;
                        if matches!(&result, Ok(true)) {
                            let s = session.clone();
                            let completion_tx = completion_tx.clone();
                            let handle = tokio::task::spawn_local(async move {
                                s.resume_plan_approval(completion_tx).await;
                            });
                            session.restored_plan_approval.arm(handle);
                        }
                        let _ = respond_to.send(result.map(|_| ()));
                    }
                    SessionCommand::QueuePrompt { prompt_id, prompt_blocks, origin, turn_kind, client_identifier, screen_mode, verbatim, json_schema, respond_to, persist_ack } => {
                        if let Err(error) = session.ensure_prefix_ready().await {
                            let boundary_error =
                                crate::session::commands::fatal_turn_boundary_error(
                                    "bootstrap",
                                    format!(
                                        "session context was not durably published: {error}"
                                    ),
                                );
                            let _ = respond_to.send(Err(boundary_error));
                            drop(persist_ack);
                            tracing::error!(
                                %error,
                                "closing session after deferred bootstrap persistence failure"
                            );
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                        // Clear suppression -- user is re-engaging
                        // (skip for synthetic auto-wake prompts; the user hasn't
                        // actually re-engaged, so post-cancel suppression must hold)
                        if !origin.is_synthetic() {
                            let mut state = session.state.lock().await;
                            state.notifications_suppressed = false;
                            ::diagnostics::unified_log::info(
                                "shell.task_wake.gate_cleared",
                                Some(session.session_info.id.0.as_ref()),
                                Some(serde_json::json!({ "reason": "user_intake" })),
                            );
                            // Layer-3 LazinessDetector wake: bump
                            // the monotonic counter so any
                            // currently-spawned classifier
                            // poll-loop snapshots a stale value
                            // and aborts. Synthetic prompts
                            // (NotificationDrain, Goal continuation,
                            // auto-wake) are not real user input
                            // and must NOT bump the counter.
                            // `AcqRel` (not bare `Release`): `fetch_add`
                            // is a read-modify-write — `AcqRel` publishes
                            // our write AND synchronizes the read half,
                            // so any future reader chaining off the
                            // returned counter value sees all prior
                            // writes from other threads. Costs nothing
                            // on x86, costs little on ARM.
                            session
                                .user_input_generation
                                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        }
                        if origin.is_synthetic() {
                            let state = session.state.lock().await;
                            let has_running = state.foreground.regular().is_some();
                            let queue_depth = state.pending_inputs.len();
                            drop(state);
                            tracing::info!(
                                prompt_id = %prompt_id,
                                has_running_task = has_running,
                                queue_depth = queue_depth,
                                "auto-wake: session actor received synthetic prompt"
                            );
                        }
                        session
                            .queue_input(prompt_blocks, prompt_id, origin, turn_kind, client_identifier, screen_mode, verbatim, json_schema, respond_to, persist_ack)
                            .await;
                        if !maybe_start_pending_manual_compaction(session.clone()).await
                        {
                            SessionActor::maybe_start_running_task(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                    }
                    SessionCommand::ExecuteSlashCommand { invocation, respond_to } => {
                        let result = session
                            .execute_out_of_band_slash_command(invocation)
                            .await;
                        if let Ok(Some(control)) = result.as_ref() {
                            // Preserve steering that reached the interjection
                            // buffer before the control invalidated this turn.
                            if let Err(error) = session
                                .cancel_turn_for_goal_control(control, &mut replay_buffer)
                                .await
                            {
                                let _ = respond_to.send(Err(format!("{error:?}")));
                                terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                                return;
                            }
                        }
                        session.apply_pending_step_controls_if_idle().await;
                        if !maybe_start_pending_manual_compaction(session.clone()).await
                        {
                            SessionActor::maybe_start_running_task(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                        let _ = respond_to.send(result.map(|_| ()));
                    }
                    SessionCommand::SetSessionTitle { title, respond_to } => {
                        // A user title wins permanently. Taking the capability
                        // here prevents a later prompt from launching a title
                        // Sideband; an already-running Sideband still fails
                        // closed when it attempts to append after this event.
                        session.session_title_route.borrow_mut().take();
                        let result = session
                            .commit_session_title(title, chat_state::SessionTitleSource::User)
                            .await;
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::QueryPromptStatus { prompt_id, respond_to } => {
                        use crate::session::prompt_queue::PromptStatus;
                        let state = session.state.lock().await;
                        let status = if let Some(task) = state.foreground.regular()
                            && task.prompt_id == prompt_id
                        {
                            PromptStatus::Running {
                                turn_start_ms: task.turn_start_ms,
                            }
                        } else if let Some((position, item)) = state
                            .pending_inputs
                            .iter()
                            .enumerate()
                            .find(|(_, item)| item.prompt_id == prompt_id)
                        {
                            PromptStatus::Queued {
                                position,
                                queue_version: item
                                    .queue_meta
                                    .as_ref()
                                    .map_or(0, |meta| meta.version),
                            }
                        } else if let Some(terminal) = state
                            .recent_terminals
                            .iter()
                            .rev()
                            .find(|terminal| terminal.prompt_id == prompt_id)
                        {
                            PromptStatus::Terminal {
                                stop_reason: terminal.stop_reason.clone(),
                                agent_result: terminal.agent_result.clone(),
                            }
                        } else {
                            PromptStatus::Unknown
                        };
                        let _ = respond_to.send(status);
                    }
                    SessionCommand::QueryForeground { respond_to } => {
                        let state = session.state.lock().await;
                        let _ = respond_to.send(state.foreground.snapshot());
                    }
                    SessionCommand::ReceiveNotification {
                        source,
                        source_version,
                        body,
                        respond_to,
                    } => {
                        let deferred_subject = match &source {
                            chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                                Some(task_id.clone())
                            }
                            chat_state::NotificationSource::SubagentCompleted { subagent_id, .. } => {
                                Some(subagent_id.clone())
                            }
                            chat_state::NotificationSource::MonitorProgress { .. }
                            | chat_state::NotificationSource::TaskStillRunning { .. }
                            | chat_state::NotificationSource::WorkflowCompleted { .. } => None,
                        };
                        let admission = session
                            .receive_notification(source, source_version, body.clone())
                            .await;
                        match &admission {
                            Ok(notification_id) => {
                                if let Some(subject) = deferred_subject
                                    && session.completion_delivery.complete(subject.clone())
                                {
                                    tracing::info!(
                                        task_id = subject,
                                        notification_id,
                                        "deferred completion is durably ready"
                                    );
                                }
                                SessionActor::maybe_drain_notifications(
                                    session.clone(),
                                    completion_tx.clone(),
                                )
                                .await;
                            }
                            Err(error) => {
                                tracing::error!(%error, "notification admission failed");
                            }
                        }
                        if let Some(respond_to) = respond_to {
                            let _ = respond_to.send(admission);
                        }
                    }
                    SessionCommand::BehaviorChange { session_mode, intent, responds_to } => {
                        if session
                            .admit_behavior_selection(session_mode, intent, responds_to)
                            .await
                        {
                            let worker = session.clone();
                            let completion_tx = completion_tx.clone();
                            let handle = tokio::task::spawn_local(async move {
                                let drained = std::panic::AssertUnwindSafe(
                                    worker.clone().drain_behavior_selections(completion_tx),
                                )
                                .catch_unwind()
                                .await;
                                let failure = match drained {
                                    Ok(Ok(())) => return Ok(()),
                                    Ok(Err(())) => {
                                        "Behavior control worker crossed a fatal persistence boundary"
                                            .to_string()
                                    }
                                    Err(payload) => {
                                        let panic = payload
                                            .downcast_ref::<&str>()
                                            .copied()
                                            .or_else(|| {
                                                payload
                                                    .downcast_ref::<String>()
                                                    .map(String::as_str)
                                            })
                                            .unwrap_or("non-string panic payload");
                                        format!("Behavior control worker panicked: {panic}")
                                    }
                                };
                                {
                                    let mut state = worker.state.lock().await;
                                    state.termination.request(TerminationState::Fatal);
                                    state.behavior_control_worker_active = false;
                                    state.applying_behavior_control = None;
                                    if state.behavior_control_foreground_claimed
                                        && matches!(
                                            state.foreground,
                                            ForegroundState::ApplyingControl
                                        )
                                    {
                                        state.foreground = ForegroundState::Idle;
                                    }
                                    state.behavior_control_foreground_claimed = false;
                                }
                                worker.idle_arbiter.notify_waiters();
                                let _ = worker.event_tx.send(
                                    SessionEvent::ControlWorkerFailed {
                                        message: failure.clone(),
                                    },
                                );
                                Err(failure)
                            });
                            session.behavior_control_worker.arm(handle);
                        }
                    }
                    SessionCommand::GoalControl { command } => {
                        session.handle_goal_command(command).await;
                    }
                    SessionCommand::RecordGoalUsage { goal_id, tokens, respond_to } => {
                        let _control = session.step_control_gate.lock().await;
                        let result = session.apply_captured_goal_usage(&goal_id, tokens).await;
                        let truly_idle = {
                            let admission = session.state.lock().await;
                            admission.foreground.is_idle()
                                && admission.pending_step_controls.is_empty()
                        };
                        if matches!(result, Ok(true)) && truly_idle {
                            let _ = session.enforce_goal_spending_limit().await;
                        } else if matches!(result, Ok(false)) {
                            tracing::debug!(
                                %goal_id,
                                tokens,
                                "discarded Goal usage for a retired Goal identity"
                            );
                        }
                        let fatal = result.is_err();
                        let _ = respond_to.send(result);
                        drop(_control);
                        if fatal {
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                    }
                    SessionCommand::RecordGoalUsageIncomplete { goal_id, respond_to } => {
                        let _control = session.step_control_gate.lock().await;
                        let result = session
                            .apply_captured_goal_usage_incomplete(&goal_id)
                            .await;
                        let fatal = result.is_err();
                        let _ = respond_to.send(result);
                        drop(_control);
                        if fatal {
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                    }
                    SessionCommand::SettleGoalUsageAttempt {
                        attempt_id,
                        respond_to,
                    } => {
                        let _control = session.step_control_gate.lock().await;
                        let result = session
                            .settle_claimed_goal_usage_attempt(&attempt_id)
                            .await;
                        let truly_idle = {
                            let admission = session.state.lock().await;
                            admission.foreground.is_idle()
                                && admission.pending_step_controls.is_empty()
                        };
                        if matches!(result, Ok(true)) && truly_idle {
                            let _ = session.enforce_goal_spending_limit().await;
                        }
                        let fatal = result.is_err();
                        let _ = respond_to.send(result);
                        drop(_control);
                        if fatal {
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                    }
                    SessionCommand::SetSessionModel { route, catalog, intent, responds_to } => {
                        session
                            .admit_session_model_selection(
                                route,
                                catalog,
                                intent,
                                responds_to,
                            )
                            .await;
                    }
                    SessionCommand::PatchSessionEffort {
                        effort,
                        authority,
                        intent,
                        responds_to,
                    } => {
                        session
                            .admit_session_effort_patch(
                                effort,
                                authority,
                                intent,
                                responds_to,
                            )
                            .await;
                    }
                    SessionCommand::ReloadModelConfig {
                        catalog,
                        responds_to,
                    } => {
                        session
                            .admit_model_catalog_reload(catalog, responds_to)
                            .await;
                    }
                    SessionCommand::RebuildAgentForDefinition { definition, intent, responds_to } => {
                        session.admit_agent_selection(definition, intent, responds_to).await;
                    }
                    SessionCommand::PublishControlState { respond_to } => {
                        session.publish_control_state_snapshot().await;
                        let _ = respond_to.send(());
                    }
                    SessionCommand::GetCurrentModel { responds_to } => {
                        let model = session.current_catalog_model_id();
                        let _ = responds_to.send(model);
                    }
                    SessionCommand::GetCurrentBehavior { responds_to } => {
                        let mode = session.behavior.lock().behavior();
                        let _ = responds_to.send(mode);
                    }
                    SessionCommand::GetModelMetadata { responds_to } => {
                        let id = session.chat_state_handle.get_last_model_metadata().await;
                        let _ = responds_to.send(id);
                    }
                    SessionCommand::GetSessionInfo { responds_to } => {
                        let info = session.build_session_info().await;
                        let _ = responds_to.send(info);
                    }
                    SessionCommand::BackgroundForegroundCommand { tool_call_id, respond_to } => {
                        let result = session.agent.borrow().tool_bridge()
                            .background_foreground_command(&tool_call_id)
                            .await;
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::KillBackgroundTask { task_id, respond_to } => {
                        let result = session.agent.borrow().tool_bridge()
                            .kill_background_task(&task_id)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::DeleteScheduledTask { task_id, respond_to } => {
                        let result = session.agent.borrow().tool_bridge()
                            .delete_scheduled_task(&task_id)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::ListTasks { respond_to } => {
                        let result = session.agent.borrow().tool_bridge()
                            .list_tasks()
                            .await;
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::GetHooksList { respond_to } => {
                        use crate::extensions::hooks::hook_spec_to_info;

                        let hooks = match &*session.hooks.registry.borrow() {
                            Some(registry) => registry
                                .all_hooks()
                                .iter()
                                .map(|spec| hook_spec_to_info(spec))
                                .collect(),
                            None => Vec::new(),
                        };

                        // Report the folder-trust verdict so the flag matches
                        // the gated registry built above.
                        let project_trusted =
                            crate::agent::folder_trust::project_scope_allowed(
                                std::path::Path::new(&session.session_info.cwd),
                            );

                        let _ = respond_to.send(extension_types::HooksListResponse {
                            hooks,
                            project_trusted,
                            load_errors: session.hooks.load_errors.borrow().clone(),
                        });
                    }
                    SessionCommand::HooksAction { action, respond_to } => {
                        let outcome = session.handle_hooks_action(action).await;
                        let _ = respond_to.send(outcome);
                    }
                    SessionCommand::NotifyPluginUpdates { updates } => {
                        session
                            .send_grow_notification(
                                GrowSessionUpdate::PluginUpdatesInstalled { updates },
                            )
                            .await;
                    }
                    SessionCommand::PluginsAction { action, respond_to } => {
                        let outcome = session.handle_plugins_action(action).await;
                        let _ = respond_to.send(outcome);
                    }
                    SessionCommand::PluginsList { respond_to } => {
                        let _ = respond_to.send(session.plugin_registry.read().clone());
                    }
                    SessionCommand::DispatchNotificationHook {
                        notification_type,
                        message,
                        title,
                        level,
                    } => {
                        session
                            .dispatch_notification_hook(
                                &notification_type,
                                message,
                                title,
                                level,
                            )
                            .await;
                    }
                    SessionCommand::RecordGoalOwnedTaskIds {
                        goal_id,
                        definition_revision,
                        task_ids,
                    } => {
                        session.record_goal_owned_task_ids(&goal_id, definition_revision, task_ids);
                    }
                    SessionCommand::RemoveQueuedPrompt { id, expected_version, owner } => {
                        session.handle_remove_queued_prompt(&id, expected_version, owner.as_deref()).await;
                    }
                    SessionCommand::ReorderQueue { ordered_ids } => {
                        session.handle_reorder_queue(&ordered_ids).await;
                    }
                    SessionCommand::ClearQueue { owner } => {
                        session.handle_clear_queue(owner.as_deref()).await;
                    }
                    SessionCommand::EditQueuedPrompt { id, new_text, editor } => {
                        session.handle_edit_queued_prompt(&id, new_text, editor.as_deref()).await;
                    }
                    SessionCommand::HoldCombineEdit { id } => {
                        let mut state = session.state.lock().await;
                        state.combine_edit_holds.insert(id);
                    }
                    SessionCommand::ReleaseCombineEdit { id } => {
                        let mut state = session.state.lock().await;
                        state.combine_edit_holds.remove(&id);
                    }
                    SessionCommand::SteerQueuedPrompt { expected_turn_id, id, expected_version, owner, new_text } => {
                        session.handle_steer_queued_prompt(&expected_turn_id, &id, expected_version, owner.as_deref(), new_text.as_deref()).await;
                    }
                    SessionCommand::Cancel {
                        cancel_subagents,
                        kill_background_tasks,
                        rewind_if_pristine,
                        pause_goal,
                        trigger,
                    } => {
                        // Flush the actor-owned replay buffer before tearing
                        // down the running turn so any streamed chunks
                        // (notably AgentThoughtChunk reasoning text) still
                        // pending at cancel time are committed to
                        // updates.jsonl. Without this, the tail of a long
                        // reasoning stream sitting in the buffer when the
                        // user hits Ctrl+C would never reach local persistence.
                        if let Some(notification) = replay_buffer.flush() {
                            session.emit_buffered(notification).await;
                        }
                        // Cancellation terminates the exact turn named when a
                        // steer was admitted. Never leak residual steering to
                        // the next user turn or Goal continuation.
                        session
                            .discard_residual_interjections_at_turn_end()
                            .await;
                        let suppress_task_wakes = trigger.as_deref() == Some("ctrl_c");
                        let cancel_result = session
                            .cancel_running_task(
                                cancel_subagents,
                                kill_background_tasks,
                                rewind_if_pristine,
                                trigger,
                            )
                            .await;

                        if let Err(error) = cancel_result {
                            tracing::error!(?error, "closing session after fatal cancel boundary failure");
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }

                        // Auto-pause the active Goal ONLY on an explicit
                        // user "Pause goal" intent (the Goal interrupt
                        // panel's choice carries `pause_goal: true`). Every
                        // other cancel — plain Esc/Ctrl+C outside Goal,
                        // StopTurnOnly / StopTurnAndSubagents, subagent
                        // teardown, lifecycle shutdown — leaves an active
                        // Goal untouched (it stays Active and may be
                        // continued by the next user input).
                        session.maybe_auto_pause_goal_on_cancel(pause_goal).await;

                        // Accepted model/effort/Agent controls own the first
                        // released turn boundary, exactly as on normal turn
                        // completion. Only after they settle may compaction or
                        // a queued prompt capture the next route/harness.
                        session.apply_pending_step_controls_if_idle().await;

                        // Manual compaction admitted during the cancelled
                        // turn owns the next foreground slot; otherwise
                        // promote the queued prompt normally.
                        if !maybe_start_pending_manual_compaction(session.clone()).await
                        {
                            SessionActor::maybe_start_running_task(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                        // Ctrl+C leaves pending notifications suppressed. Other
                        // cancel triggers leave the actor eligible for its normal idle drain.
                        if !suppress_task_wakes {
                            SessionActor::maybe_drain_notifications(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                        // Cancellation settles the sole foreground owner. Once
                        // FIFO/manual work has had first refusal, wake the same
                        // idle arbiter used by normal completion so an Active
                        // Goal cannot become dormant after Stop Turn Only.
                        session.idle_arbiter.notify_one();
                    }
                    SessionCommand::CompactSession { user_context, respond_to } => {
                        session
                            .admit_manual_compaction(user_context, Some(respond_to))
                            .await;
                    }
                    SessionCommand::ReloadPlugins { registry } => {
                        // Eager fan-out: a plugin was added/removed/reloaded
                        // in another session. Adopt the pushed snapshot so this
                        // session's hooks, MCP, skills, and the client's
                        // slash-command catalog match — the same refresh the
                        // originating session gets, so switching here needs no
                        // lazy refetch. Subagents inherit the parent registry.
                        if !session.startup_hints.is_subagent {
                            // Fan-outs rebuild without per-session `_meta.pluginDirs`;
                            // re-merge this session's own dirs before adopting.
                            let registry = session.preserve_session_plugin_dirs(registry);
                            session.apply_plugin_registry_snapshot(registry).await;
                        }
                    }
                    SessionCommand::ReloadHooks => {
                        // Re-discover the session's project hooks on the
                        // now-flipped folder-trust verdict (e.g. after an
                        // interactive trust grant). Reuses the same path as
                        // `/hooks reload`; subagents inherit via the parent.
                        // Run INLINE on the serialized command loop (not a
                        // spawned task) like `ReloadPlugins`: `reload_hooks_impl`
                        // mutates `hook_registry`, and this actor's safety
                        // invariant (file-header `await_holding_refcell_ref`
                        // allow) is "no concurrent mutation" of it — spawning
                        // would race turn tasks.
                        if !session.startup_hints.is_subagent {
                            let _ = session.reload_hooks_impl().await;
                        }
                    }
                    SessionCommand::RefreshSkillBaseline => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("refresh_skill_baseline")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let cwd = s.tool_context.cwd.as_path().to_string_lossy();
                            let skills_config = crate::util::config::load_config().await.skills;
                            let pr = s.plugin_registry.read().clone();
                            let new_skills = agent::prompt::skills::list_skills_with_plugins(
                                Some(&cwd),
                                &skills_config,
                                pr.as_deref(),
                            )
                            .await;
                            tracing::info!(skills = new_skills.len(), "refreshed skill baseline after bundle sync");
                            let bridge = s.agent.borrow().tool_bridge().clone();
                            bridge.update_skill_baseline(new_skills).await;
                            if let Some(effects) = bridge.apply_pending_skill_update().await {
                                s.apply_skill_update_effects(effects).await;
                            }
                        });
                    }
                    SessionCommand::FlushMemory { respond_to } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("memory_flush_command")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            if s.memory.is_enabled() {
                                let did_flush = s.run_memory_flush("user_requested", None).await;
                                let _ = respond_to.send(Ok(did_flush));
                            } else {
                                let _ = respond_to.send(Err(
                                    acp::Error::invalid_request()
                                        .data("memory is not enabled for this session".to_string())
                                ));
                            }
                        });
                    }
                    SessionCommand::SetPermissionMode { mode } => {
                        let was = session.permissions.mode();
                        let mode = if mode.is_auto()
                            && !crate::util::config::auto_permission_mode_enabled_from_disk()
                        {
                            crate::util::config::PermissionMode::Ask
                        } else {
                            mode
                        };
                        tracing::info!(?mode, "Session received SetPermissionMode");
                        session.permissions.set_mode(mode);
                        let actual = session.permissions.mode();
                        if permission_mode_change(was, actual).is_some() {
                            session.emit_event(
                                crate::session::events::Event::PermissionModeChanged {
                                    previous_mode: was,
                                    mode: actual,
                                },
                            );
                        }
                        if actual.is_auto() {
                            session.wire_permission_auto_llm_classifier().await;
                        } else {
                            session.permissions.set_llm_side_query_wired(false);
                        }
                    }
                    SessionCommand::ResetPermissionState => {
                        session.permissions.reset_state();
                        tracing::info!(
                            session_id = %session.session_info.id,
                            "Permission state reset via notification"
                        );
                    }
                    SessionCommand::Rewind { request, respond_to } => {
                        let result = session.handle_rewind(request).await;
                        let transaction_incomplete = result.is_err();
                        let _ = respond_to.send(result);
                        if transaction_incomplete {
                            session.goal_drive.cancel();
                            tracing::error!(
                                session_id = %session.session_info.id,
                                "rewind transaction is incomplete; stopping the actor for recovery"
                            );
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                    }
                    SessionCommand::RepairHistory { dry_run, respond_to } => {
                        let result = session.handle_repair_history(dry_run).await;
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::GetRewindPoints { respond_to } => {
                        let response = session.get_rewind_points().await;
                        let _ = respond_to.send(response);
                    }
                    SessionCommand::GetRewindFileCounts { respond_to } => {
                        let _ = respond_to.send(session.rewind_file_counts().await);
                    }
                    SessionCommand::GrowSessionNotification { notification } => {
                        session.handle_grow_session_notification(notification).await;
                    }
                    SessionCommand::RecordSubagentUsage {
                        subagent_id,
                        by_model,
                        parent_prompt_id,
                        incomplete,
                        respond_to,
                    } => {
                        use super::updates::SubagentUsageApply;
                        match session
                            .record_subagent_usage(
                                &subagent_id,
                                &by_model,
                                parent_prompt_id.as_deref(),
                                incomplete,
                            )
                            .await
                        {
                            Ok(SubagentUsageApply::AttributedToPrompt) => {
                                // Any nested incomplete is already on the ledger;
                                // no sticky mark needed.
                                let _ = respond_to.send(());
                            }
                            Ok(SubagentUsageApply::SessionOnly) => {
                                // Report-level sticky: the stamped prompt's bill
                                // under-counts.
                                let _ = session
                                    .mark_subagent_usage_not_applied(
                                        parent_prompt_id.as_deref(),
                                    )
                                    .await;
                                let _ = respond_to.send(());
                            }
                            // Drop oneshot → fold_acked=false on child; true-miss path runs.
                            Err(()) => {}
                        }
                    }
                    SessionCommand::MarkSubagentUsageNotApplied {
                        parent_prompt_id,
                        respond_to,
                    } => {
                        // True apply-miss: sticky + pin-aware ledger fail-closed.
                        if session
                            .mark_apply_miss_incomplete(parent_prompt_id.as_deref())
                            .await
                        {
                            let _ = respond_to.send(());
                        }
                    }
                    SessionCommand::ErrorPathUsageFallback {
                        prompt_id,
                        respond_to,
                    } => {
                        let pid = prompt_id.or_else(|| {
                            session
                                .current_prompt_id
                                .lock()
                                .ok()
                                .and_then(|g| g.clone())
                        });
                        let usage = match pid.as_deref() {
                            Some(id) => session.error_path_usage_fallback(id).await,
                            None => {
                                match session.chat_state_handle.try_get_prompt_usage().await {
                                    Ok(ledger) => {
                                        crate::extensions::notification::PromptUsage::for_error_path(
                                            ledger.as_ref(),
                                            false,
                                        )
                                    }
                                    Err(()) => {
                                        crate::extensions::notification::PromptUsage::for_error_path(
                                            None, true,
                                        )
                                    }
                                }
                            }
                        };
                        let _ = respond_to.send(usage);
                    }
                    SessionCommand::FlushComplete { respond_to } => {
                        // Flush the actor-owned replay buffer inline. This branch
                        // already runs inside `run_session()`, so sending a replay
                        // flush event to `event_tx` would deadlock waiting for the
                        // same loop to process its own mailbox.
                        if let Some(notification) = replay_buffer.flush() {
                            session.emit_buffered(notification).await;
                        }
                        // Chain through persistence actor — only signal after
                        // flush_pending() completes on disk. This makes
                        // FlushComplete a true sync barrier (unlike the old
                        // pattern which signaled before the persistence actor
                        // processed the flush).
                        let _ = session
                            .notifications.persistence_tx
                            .send(PersistenceMsg::FlushAndAck { respond_to });
                    }
                    SessionCommand::UpdateMcpServers { mcp_servers, respond_to } => {
                        if session.startup_hints.is_subagent {
                            tracing::debug!(
                                session_id = %session.session_info.id.0,
                                "Skipping UpdateMcpServers for subagent session",
                            );
                            let _ = respond_to.send(Ok(()));
                            continue;
                        }
                        tracing::info!(
                            "Updating MCP servers for session '{}' ({} servers)",
                            session.session_info.id.0,
                            mcp_servers.len()
                        );

                        // Re-seed the session-scoped MCP output cap
                        // (repo `[mcp] max_output_bytes`) BEFORE the
                        // unchanged-diff early-exit below: this command
                        // also fires for `<cwd>/.grow/config.toml` edits,
                        // and a cap-only edit changes no server configs.
                        session.reseed_mcp_output_cap().await;

                        // Capture the dispatcher's
                        // event sender alongside the diff so we
                        // can fan out `McpClientEvent::ConfigDiff`
                        // immediately after the in-memory swap
                        // completes — without holding the
                        // `mcp_state` lock across the emit.
                        let (diff, dispatch_event_tx) = {
                            let mut mcp_state = session.mcp_state.lock().await;
                            let diff = mcp_state.update_configs_diff(mcp_servers);
                            let tx = mcp_state.client_event_tx();
                            (diff, tx)
                        };

                        let Some(diff) = diff else {
                            tracing::debug!(
                                "MCP configs unchanged for session '{}', skipping re-initialization",
                                session.session_info.id.0
                            );
                            let _ = respond_to.send(Ok(()));
                            continue;
                        };

                        // Emit one `ConfigDiff` so the
                        // `StatusDispatcher` fans out per-server
                        // `mcp/server_status` with
                        // `reason: ConfigAdded` / `ConfigRemoved`.
                        // Best-effort — a dropped dispatcher
                        // means `mcp.liveness_watchers` is
                        // off or the session has shut down; the
                        // tool-bridge tear-down and re-init below
                        // still happen.
                        if (!diff.added.is_empty() || !diff.removed.is_empty())
                            && let Some(tx) = &dispatch_event_tx
                        {
                            let _ = tx.send(
                                ::mcp::servers::McpClientEvent::ConfigDiff {
                                    added: diff.added.clone(),
                                    removed: diff.removed.clone(),
                                },
                            );
                        }

                        for name in &diff.removed {
                            let prefix = format!(
                                "{}{}",
                                name,
                                workspace_types::MCP_TOOL_NAME_DELIMITER
                            );
                            let removed_count = session
                                .agent
                                .borrow()
                                .tool_bridge()
                                .unregister_tools_by_prefix(&prefix);
                            tracing::info!(
                                server = name.as_str(),
                                tools_removed = removed_count,
                                "Unregistered tools for removed MCP server"
                            );
                        }

                        let session_for_mcp = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("update_mcp_servers")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            session_for_mcp.ensure_mcp_tools_initialized().await;
                            let _ = respond_to.send(Ok(()));
                        });
                    }
                    SessionCommand::ToggleMcpServer { server_name, enabled, server_config, respond_to } => {
                        let mut mcp_state = session.mcp_state.lock().await;
                        let mut configs = mcp_state.configs.clone();

                        if enabled {
                            if let Some(config) = server_config {
                                // Replace any prior entry so setup → enable can
                                // swap an unresolved placeholder for a resolved URL.
                                configs.retain(|c| {
                                    crate::session::mcp_servers::mcp_server_name(c)
                                        != server_name
                                });
                                configs.push(config);
                            } else {
                                let already_present = configs.iter().any(|c| {
                                    crate::session::mcp_servers::mcp_server_name(c)
                                        == server_name
                                });
                                if already_present {
                                    drop(mcp_state);
                                    let _ = respond_to.send(Ok(()));
                                    continue;
                                }
                                drop(mcp_state);
                                let _ = respond_to.send(Err(acp::Error::invalid_params()
                                    .data(format!("server '{}' not found in config", server_name))));
                                continue;
                            }
                        } else {
                            configs.retain(|c| crate::session::mcp_servers::mcp_server_name(c) != server_name);
                        }

                        let diff = mcp_state.update_configs_diff(configs);
                        // Snapshot the dispatcher
                        // sender BEFORE dropping the lock so the
                        // emit below survives any later mutation.
                        let dispatch_event_tx = mcp_state.client_event_tx();
                        drop(mcp_state);

                        let Some(diff) = diff else {
                            let _ = respond_to.send(Ok(()));
                            continue;
                        };

                        // ToggleMcpServer mirrors
                        // UpdateMcpServers — fan out per-server
                        // status via the dispatcher (`ConfigAdded`
                        // / `ConfigRemoved` reason codes on
                        // `mcp/server_status`).
                        if (!diff.added.is_empty() || !diff.removed.is_empty())
                            && let Some(tx) = &dispatch_event_tx
                        {
                            let _ = tx.send(
                                ::mcp::servers::McpClientEvent::ConfigDiff {
                                    added: diff.added.clone(),
                                    removed: diff.removed.clone(),
                                },
                            );
                        }

                        for name in &diff.removed {
                            let prefix = format!(
                                "{}{}",
                                name,
                                workspace_types::MCP_TOOL_NAME_DELIMITER
                            );
                            let removed_count = session
                                .agent
                                .borrow()
                                .tool_bridge()
                                .unregister_tools_by_prefix(&prefix);
                            tracing::info!(
                                server = name.as_str(),
                                tools_removed = removed_count,
                                "Unregistered tools for toggled MCP server"
                            );
                        }

                        let session_for_mcp = session.clone();
                        let sname = server_name.clone();
                        let session_cwd = session.session_info.cwd.clone();
                        let activity = session
                            .session_activities
                            .try_start("toggle_mcp_server")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            session_for_mcp.ensure_mcp_tools_initialized().await;
                            if let Err(e) = crate::util::config::save_mcp_server_enabled_in(
                                &sname,
                                enabled,
                                std::path::Path::new(&session_cwd),
                            )
                            .await
                            {
                                tracing::warn!(
                                    server = sname.as_str(),
                                    error = %e,
                                    "Failed to persist server enabled state to config"
                                );
                            }
                            let _ = respond_to.send(Ok(()));
                        });
                    }
                    SessionCommand::ToggleMcpTool { server_name, tool_name, enabled, respond_to } => {
                        let qualified = format!(
                            "{}{}{}",
                            server_name,
                            workspace_types::MCP_TOOL_NAME_DELIMITER,
                            tool_name,
                        );
                        let mut mcp_state = session.mcp_state.lock().await;

                        if enabled {
                            // Re-enable: remove from disabled set, re-register from stashed registration.
                            if let Some(set) = mcp_state.disabled_tools.get_mut(&server_name) {
                                set.remove(&tool_name);
                                if set.is_empty() {
                                    mcp_state.disabled_tools.remove(&server_name);
                                }
                            }
                            if let Some(reg) = mcp_state.disabled_tool_registrations.remove(&qualified)
                                && reg.model_visible
                            {
                                let bridge = session.agent.borrow().tool_bridge().clone();
                                if let Err(e) = bridge
                                    .register_mcp_tools(reg.name, reg.tool, Some(reg.input_schema))
                                    .await
                                {
                                    tracing::warn!(
                                        tool = qualified.as_str(),
                                        error = %e,
                                        "Failed to re-register toggled MCP tool"
                                    );
                                }
                            }
                        } else {
                            // Disable: stash a registration so the tool can be
                            // re-enabled without a full re-init, then unregister.
                            let bridge = session.agent.borrow().tool_bridge().clone();
                            let tool_def = bridge
                                .tool_definitions()
                                .await
                                .into_iter()
                                .find(|d| d.function.name == qualified);
                            if let Some(def) = tool_def {
                                let meta = mcp_state.mcp_tool_meta.get(&qualified).cloned();
                                let schema = def.function.parameters.clone();
                                let mcp_tool = crate::session::mcp_servers::McpTool::new(
                                    tool_name.clone(),
                                    def.function.description.clone().unwrap_or_default(),
                                    server_name.clone(),
                                    session.mcp_state.clone(),
                                    schema,
                                    meta,
                                );
                                if let Some(reg) = mcp_tool.into_registration() {
                                    mcp_state
                                        .disabled_tool_registrations
                                        .insert(qualified.clone(), reg);
                                }
                            }
                            bridge.unregister_tool_by_name(&qualified);
                            mcp_state
                                .disabled_tools
                                .entry(server_name.clone())
                                .or_default()
                                .insert(tool_name.clone());
                        }

                        // Collect the new disabled set for this server before dropping lock.
                        let disabled_vec: Vec<String> = mcp_state
                            .disabled_tools
                            .get(&server_name)
                            .map(|s| s.iter().cloned().collect())
                            .unwrap_or_default();
                        drop(mcp_state);

                        session.refresh_mcp_snapshot_and_schedule_reminder().await;
                        session.refresh_goal_runtime_availability().await;

                        // Persist to config and emit notification in background.
                        let notifications = session.notifications.gateway.clone();
                        let session_id = session.session_info.id.0.clone();
                        let server_for_persist = server_name.clone();
                        let activity = session
                            .session_activities
                            .try_start("toggle_mcp_tool")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            if let Err(e) = crate::util::config::save_mcp_disabled_tools(
                                &server_for_persist,
                                &disabled_vec,
                            ).await {
                                tracing::warn!(
                                    server = server_for_persist.as_str(),
                                    error = %e,
                                    "Failed to persist disabled_tools to config"
                                );
                            }
                            // The persisted disable mask changed the canonical
                            // catalog. The producer does not retain a complete
                            // UI projection here, so the absent `tools` field
                            // asks the client to refresh from `mcp/list`.
                            let payload = crate::extensions::mcp::McpServerStatusPayload {
                                session_id: session_id.to_string(),
                                name: server_for_persist,
                                status: crate::extensions::mcp::McpServerStatus::Ready,
                                reason: crate::extensions::mcp::McpServerStatusReason::ConfigChanged,
                                detail: None,
                                tools: None,
                            };
                            if let Ok(params) =
                                serde_json::value::to_raw_value(&payload)
                            {
                                notifications.forward_fire_and_forget(acp::ExtNotification::new(
                                    crate::extensions::mcp::SERVER_STATUS_METHOD,
                                    params.into(),
                                ));
                            }
                            let _ = respond_to.send(Ok(()));
                        });
                    }
                    SessionCommand::SnapshotMcpPool { respond_to } => {
                        let mcp_state = session.mcp_state.lock().await;
                        // Preserve the live authority even when the catalog is
                        // empty at spawn. Otherwise a child created before a
                        // parent server connects can never observe that later
                        // eligible addition.
                        let _ = respond_to.send(Some(
                            crate::session::mcp_servers::SharedMcpPool::from_state(&mcp_state),
                        ));
                    }
                    SessionCommand::SnapshotClientHooks { respond_to } => {
                        let _ = respond_to.send(session.hooks.client_hooks.borrow().clone());
                    }
                    SessionCommand::SetClientHooks { hooks } => {
                        *session.hooks.client_hooks.borrow_mut() = hooks;
                    }
                    SessionCommand::GetMcpStatus { respond_to } => {
                        let mcp_state = session.mcp_state.clone();
                        let tool_bridge = session.agent.borrow().tool_bridge().clone();
                        let activity = session
                            .session_activities
                            .try_start("get_mcp_status")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let snapshot = crate::extensions::mcp::build_mcp_status(
                                &mcp_state,
                                &tool_bridge,
                            ).await;
                            let _ = respond_to.send(snapshot);
                        });
                    }
                    SessionCommand::CallMcpTool { server_name, server_url, tool_name, arguments, respond_to } => {
                        let mcp_state = session.mcp_state.clone();
                        let activity = session
                            .session_activities
                            .try_start("call_mcp_tool")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = crate::extensions::mcp::call_mcp_tool(
                                &mcp_state,
                                &server_name,
                                server_url.as_deref(),
                                &tool_name,
                                arguments,
                            ).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::ReadMcpResource { server_name, uri, respond_to } => {
                        let mcp_state = session.mcp_state.clone();
                        let activity = session
                            .session_activities
                            .try_start("read_mcp_resource")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = crate::extensions::mcp::read_mcp_resource(
                                &mcp_state,
                                &server_name,
                                &uri,
                            ).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::AdvertiseCommands => {
                        session.send_available_commands_update().await;
                    }
                    SessionCommand::GetWorkflowCatalogState { respond_to } => {
                        let tool_names = session.registered_tool_names().await;
                        let has_runs = !session.workflow_tracker().await.lock().list().is_empty();
                        let availability =
                            session.build_command_availability(&tool_names, has_runs);
                        let _ = respond_to
                            .send((availability.workflows, availability.workflow_management));
                    }
                    SessionCommand::ListAvailableCommands { respond_to } => {
                        let skills = session.slash_skills_for_resolve().await;
                        let tool_names = session.registered_tool_names().await;
                        let has_runs = !session.workflow_tracker().await.lock().list().is_empty();
                        let availability =
                            session.build_command_availability(&tool_names, has_runs);
                        let (_, workflows, _) = session.named_workflow_snapshot();
                        let commands = slash_commands::available_commands(
                            &skills,
                            availability,
                            &workflows,
                        );
                        let _ = respond_to.send(slash_commands::ListCommandsResponse {
                            commands,
                            tools: Some(tool_names),
                        });
                    }
                    SessionCommand::ReloadSkills => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("reload_skills")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            s.reload_skills_from_disk().await;
                        });
                    }
                    SessionCommand::DispatchSessionStartHook { source } => {
                        let envelope = session.fire_hook(
                            ::hooks::event::HookEventName::SessionStart,
                            None,
                            ::hooks::event::HookPayload::SessionStart {
                                source,
                                model_id: None,
                                agent_type: None,
                            },
                        );
                        if let Some(registry) = session.hooks.registry.borrow().clone() {
                            let ctx = session.hook_run_ctx();
                            let results = ::hooks::dispatcher::dispatch_non_blocking(
                                &registry,
                                ::hooks::event::HookEventName::SessionStart,
                                &envelope,
                                &ctx,
                            )
                            .await;
                            session.send_hook_execution("session_start", None, None, &results).await;
                        }
                    }
                    SessionCommand::GetActiveAgent { responds_to } => {
                        let _ = responds_to.send(Some(session.agent.borrow().name().to_owned()));
                    }
                    SessionCommand::SideQuestion { question, respond_to } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("side_question")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = s.handle_side_question(&question).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::Recap { auto } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("recap")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            s.handle_recap(auto).await;
                        });
                    }
                    SessionCommand::AISuggest { prefix, cwd, model_override, respond_to } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("ai_suggest")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = s.handle_ai_suggest(&prefix, &cwd, model_override.as_deref()).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::SuggestPrompt { model_override, respond_to } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("suggest_prompt")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = s.handle_suggest_prompt(model_override.as_deref()).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::RewriteMemoryNote { raw_text, context_summary, respond_to } => {
                        let s = session.clone();
                        let activity = session
                            .session_activities
                            .try_start("rewrite_memory_note")
                            .expect("command activity admission is open while mailbox is serviced");
                        tokio::task::spawn_local(async move {
                            let _activity = activity;
                            let result = s.handle_rewrite_memory_note(&raw_text, &context_summary).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::SteerTurn { expected_turn_id, text, id, images, respond_to } => {
                        let image_count = images.len() as u32;
                        let admitted = session
                            .admit_mid_turn_interjection(
                                &expected_turn_id,
                                text.clone(),
                                images,
                            )
                            .await;
                        if !admitted {
                            let _ = respond_to.send(Err("the target turn is no longer running".to_string()));
                            continue;
                        }
                        // Broadcast to every attached client so all panes
                        // viewing this session render the interjection block
                        // — not just the originating client. The originator
                        // dedups this echo by `id` against its optimistic
                        // local block; viewers render it.
                        session.broadcast_interjection(&text, id.as_deref());
                        // Diagnostic at enqueue (not drain) so it is recorded
                        // even when a cancel clears the buffer before the
                        // next drain point.
                        session.events.emit(crate::session::events::Event::Interjected {
                            source: crate::session::events::InterjectionSource::Direct,
                            image_count,
                            redirect_kind: crate::session::events::RedirectKind::Interjection,
                        });
                        let _ = respond_to.send(Ok(()));
                        tracing::info!(expected_turn_id, "Queued same-turn steering input");
                    }
                    SessionCommand::WorkflowCompleted {
                        state,
                        respond_to,
                    } => {
                        let run_id = state.run_id.clone();
                        let admission = session
                            .admit_public_workflow_completion(&state)
                            .await;
                        if admission.is_ok() {
                            session.send_available_commands_update().await;
                            SessionActor::maybe_drain_notifications(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                        if let Err(error) = &admission {
                            tracing::error!(run_id, %error, "workflow terminal reconciliation failed");
                        }
                        let _ = respond_to.send(admission);
                    }
                    SessionCommand::WorkflowTerminalFailure { run_id, error } => {
                        tracing::error!(
                            %run_id,
                            %error,
                            "Workflow terminal persistence crossed the session fatal sink"
                        );
                        terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                        return;
                    }
                    SessionCommand::TakeTurnMessages { respond_to } => {
                        let result = session.chat_state_handle.take_turn_messages().await;
                        let _ = respond_to.send(result);
                    }
                    SessionCommand::PersistGitHead { commit, branch } => {
                        let _ = session.notifications.persistence_tx.send(
                            PersistenceMsg::GitHead { commit, branch },
                        );
                    }
                    command @ (SessionCommand::Shutdown
                    | SessionCommand::UnloadIfIdle { .. }) => {
                        let is_shutdown = matches!(&command, SessionCommand::Shutdown);
                        let mut unload_respond_to = None;
                        if let SessionCommand::UnloadIfIdle { respond_to } = command {
                            // This decision and the termination latch are
                            // performed in one actor turn. The mailbox remains
                            // physically open only for descendant Goal usage
                            // settlement, then closes after child drain.
                            let goal_status = session.goal_tracker.lock().status();
                            let has_parked_plan_approval =
                                crate::session::pending_interaction::has_parked_plan_approval(
                                    &session.pending_interactions,
                                );
                            let busy = {
                                let state = session.state.lock().await;
                                session_has_work(
                                    &state,
                                    goal_status,
                                    has_parked_plan_approval,
                                )
                            } || !cmd_rx.is_empty();
                            // The leader bounds this transaction. If its
                            // waiter timed out while the actor was occupied,
                            // the unload request is cancelled: shutting down
                            // now would leave a dead actor behind a retained
                            // session handle.
                            if respond_to.is_closed() {
                                continue;
                            }
                            if busy {
                                let _ = respond_to.send(false);
                                continue;
                            }
                            unload_respond_to = Some(respond_to);
                        }
                        if let Err(error) = latch_termination_and_cancel_controls(
                            &session,
                            TerminationState::Graceful,
                        )
                        .await
                        {
                            tracing::error!(%error, "failed to accept graceful session shutdown");
                            if let Some(respond_to) = unload_respond_to.take() {
                                let _ = respond_to.send(false);
                            }
                                terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                        // `UnloadIfIdle` acknowledges the atomic admission
                        // decision, not completion of hooks/memory/persistence
                        // teardown. The leader can now remove the closed
                        // SessionHandle while retaining the SessionThread that
                        // supervises the bounded drain.
                        if let Some(respond_to) = unload_respond_to.take() {
                            let _ = respond_to.send(true);
                        }
                        if let Err(error) =
                            begin_graceful_shutdown(&session, &mut replay_buffer, &mut cmd_rx).await
                        {
                            tracing::error!(%error, "failed to begin graceful session shutdown");
                            terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                            return;
                        }
                        pending_shutdown = Some(GracefulShutdown {
                            reason: if is_shutdown { "shutdown" } else { "idle_unload" },
                        });
                        continue;
                    }
                }
                if session.chat_state_handle.is_closed() {
                    tracing::error!(
                        "closing session because the Timeline writer mailbox is unavailable"
                    );
                    terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                    return;
                }
            }
            _ = session.idle_arbiter.notified() => {
                if session.chat_state_handle.is_closed() {
                    tracing::error!(
                        "closing session because the Timeline writer mailbox is unavailable"
                    );
                    terminate_failed_timeline_writer(&session, &mut cmd_rx).await;
                    return;
                }
                arbitrate_idle_wake(session.clone(), completion_tx.clone()).await;
            }
        }
    };
    finish_graceful_shutdown(&session, &mut replay_buffer, &mut cmd_rx, shutdown).await;
}
