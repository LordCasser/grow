//! The session actor's main loop (`run_session`): command dispatch, idle
//! arms, and the free helpers only the loop consumes.
#![allow(clippy::items_after_test_module)]
use super::*;

fn spawn_manual_compaction(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
    user_context: Option<String>,
    respond_to: Option<
        tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
    >,
) {
    tokio::task::spawn_local(async move {
        let result = session.run_compact(user_context).await;
        if let Some(respond_to) = respond_to {
            let response = result
                .as_ref()
                .map(|_| crate::session::CompactConversationStatus::Completed)
                .map_err(Clone::clone);
            let _ = respond_to.send(response);
        } else if let Err(error) = &result {
            session
                .send_host_turn_slash_command_output(&format!(
                    "Scheduled compaction failed: {error}"
                ))
                .await;
        }
        session.state.lock().await.foreground = ForegroundState::Idle;
        SessionActor::maybe_start_running_task(session.clone(), completion_tx.clone()).await;
        SessionActor::maybe_drain_notifications(session.clone(), completion_tx).await;
        session.emit_session_idle_if_idle().await;
    });
}

async fn maybe_start_pending_manual_compaction(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
) -> bool {
    let user_context = {
        let mut state = session.state.lock().await;
        if !state.foreground.is_idle() {
            return false;
        }
        let Some(user_context) = state.pending_manual_compact.take() else {
            return false;
        };
        state.foreground = ForegroundState::Compaction;
        user_context
    };
    spawn_manual_compaction(session, completion_tx, user_context, None);
    true
}

/// Apply the complete idle-admission order for an external idle permit.
/// Restored receipts may predate the permit, so the Goal driver must never be
/// called directly from the select branch.
async fn arbitrate_idle_wake(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
) {
    if !maybe_start_pending_manual_compaction(session.clone(), completion_tx.clone()).await {
        SessionActor::maybe_start_running_task(session.clone(), completion_tx.clone()).await;
    }
    SessionActor::maybe_drain_notifications(session.clone(), completion_tx.clone()).await;
    if session.state.lock().await.foreground.is_idle() {
        session.drive_goal_on_idle(completion_tx).await;
    }
}

#[cfg(test)]
mod idle_admission_tests {
    use super::*;

    #[tokio::test]
    async fn restored_notification_wins_an_idle_permit_before_goal_continuation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::acp_session::support::build_actor().await;
                actor
                    .goal_runtime_available
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish the architecture migration".into(),
                        None,
                        "2026-08-24T00:00:00Z".into(),
                    )
                    .unwrap();
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "outside-goal".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "independent task completed".into(),
                    )
                    .await
                    .unwrap();
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                arbitrate_idle_wake(actor.clone(), completion_tx).await;

                let state = actor.state.lock().await;
                assert!(matches!(
                    state.foreground.regular().map(|task| &task.origin),
                    Some(crate::session::PromptOrigin::TaskCompleted { task_id })
                        if task_id == "outside-goal"
                ));
            })
            .await;
    }
}

/// Returns the authoritative mode only when the manager accepted a real
/// transition. Callers pass the post-clamp read-back, never the request.
pub(super) fn permission_mode_change(
    was: ::diagnostics::enums::PermissionMode,
    actual: ::diagnostics::enums::PermissionMode,
) -> Option<::diagnostics::enums::PermissionMode> {
    (was != actual).then_some(actual)
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
/// Best-effort removal of this session's per-session scratch staging on
/// teardown. A no-op in builds without a scratch producer.
fn cleanup_session_scratch(_session: &SessionActor) {}
impl SessionActor {
    /// `CompactSession` admission on the mailbox: decide the immediate
    /// status and either queue a pending compact behind the running turn or
    /// spawn the foreground compaction. Extracted from the command arm so
    /// the mailbox-not-blocked property is directly testable (the command
    /// must be accepted while a background Goal verification stage runs).
    pub(super) async fn admit_manual_compaction(
        self: &std::sync::Arc<Self>,
        user_context: Option<String>,
        completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
        respond_to: Option<
            tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
        >,
    ) {
        let mut state = self.state.lock().await;
        if state.foreground.regular().is_some() {
            let status = if state.pending_manual_compact.is_some() {
                crate::session::CompactConversationStatus::AlreadyRunning
            } else {
                state.pending_manual_compact = Some(user_context);
                crate::session::CompactConversationStatus::Scheduled
            };
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(status));
            }
        } else if matches!(state.foreground, ForegroundState::Compaction)
            || self.compaction.lease.is_in_flight()
        {
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(
                    crate::session::CompactConversationStatus::AlreadyRunning,
                ));
            }
        } else {
            state.foreground = ForegroundState::Compaction;
            drop(state);
            spawn_manual_compaction(
                std::sync::Arc::clone(self),
                completion_tx,
                user_context,
                respond_to,
            );
        }
    }
}

async fn shutdown_workflows(session: &SessionActor) {
    session.checkpoint_goal_before_shutdown().await;
    if let Err(run_ids) = session
        .workflow_manager
        .lock()
        .await
        .cancel_all_and_drain(std::time::Duration::from_secs(7))
        .await
    {
        tracing::warn!(
            ?run_ids,
            "workflow shutdown completed with interrupted runs"
        );
    }
    let (respond_to, ack) = tokio::sync::oneshot::channel();
    if session
        .notifications
        .persistence_tx
        .send(PersistenceMsg::FlushAndAck { respond_to })
        .is_err()
    {
        tracing::warn!("workflow shutdown persistence channel closed before flush");
        return;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            tracing::warn!("workflow shutdown persistence actor dropped flush ack")
        }
        Err(_) => tracing::warn!("workflow shutdown persistence flush timed out"),
    }
}

/// Close the primary-owned permission authority at teardown entry and wait
/// until its final audit event has crossed the bridge. End hooks and memory
/// work may take time; no child permission request remains live while they run.
async fn stop_permission_manager_and_drain_audit(session: &SessionActor) {
    if session.owns_permission_manager {
        session.permissions.shutdown_and_drain().await;
        let bridge = session.permission_audit_bridge.lock().take();
        if let Some(bridge) = bridge
            && let Err(error) = bridge.await
        {
            tracing::warn!(%error, "permission audit bridge failed during session shutdown");
        }
    }
}

/// Cross the final persistence barrier after every teardown producer,
/// including the drained permission bridge and session-end hooks, has stopped.
async fn final_session_persistence_flush(session: &SessionActor) {
    let (respond_to, ack) = tokio::sync::oneshot::channel();
    if session
        .notifications
        .persistence_tx
        .send(PersistenceMsg::FlushAndAck { respond_to })
        .is_err()
    {
        tracing::warn!("permission audit persistence channel closed before final flush");
        return;
    }
    if ack.await.is_err() {
        tracing::warn!("permission audit persistence actor dropped flush ack");
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
                let _ =
                    event_tx_for_flush_timer.send(SessionEvent::FlushReplay { respond_to: None });
            }
        });
    }
    let _workflow_watch = crate::config::watcher::ProjectDiscoveryWatcher::start(
        std::path::Path::new(session.session_info.cwd.as_str()),
    )
    .map(|(mut watcher, mut changes)| {
        let session = session.clone();
        tokio::task::spawn_local(async move {
            while let Some(change) = changes.recv().await {
                watcher.refresh_new_dirs();
                match change {
                    crate::config::watcher::DiscoveryChange::Skills => {
                        session.reload_skills_from_disk().await;
                    }
                    crate::config::watcher::DiscoveryChange::Workflows => {
                        session.send_available_commands_update().await;
                    }
                }
            }
        })
    });
    let _fs_watch: Option<fs_watch::FsWatchHandle> = if fs_watch_caps.needs_watcher() {
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
    {
        let s = session.clone();
        tokio::task::spawn_local(async move { s.maybe_notify_git_branch().await });
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
        tokio::task::spawn_local(async move {
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
    }
    let session_for_mcp = session.clone();
    let completion_tx_for_mcp = completion_tx.clone();
    tokio::task::spawn_local(async move {
        session_for_mcp.ensure_mcp_tools_initialized().await;
        SessionActor::maybe_start_running_task(
            session_for_mcp.clone(),
            completion_tx_for_mcp.clone(),
        )
        .await;
        SessionActor::maybe_drain_notifications(session_for_mcp, completion_tx_for_mcp).await;
    });
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
    loop {
        tokio::select! {
            biased;
            // Idle flush timer fired — run background flush.
            _ = &mut idle_flush_sleep, if session.idle_flush_timeout.is_some()
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
                    tokio::task::spawn_local({
                        let session = session.clone();
                        async move {
                            if !session.run_memory_flush("interval", None).await {
                                tracing::info!(target: ::diagnostics::memory_log::TARGET,
                                    "MEMORY_IDLE_FLUSH: skipped — another flush already in progress");
                            }
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
                && session.memory.is_enabled() => {
                tracing::debug!(target: ::diagnostics::memory_log::TARGET,
                    "MEMORY_DREAM_CHECK: timer fired");
                tokio::task::spawn_local({
                    let session = session.clone();
                    async move {
                        session.maybe_run_dream().await;
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
                    Some(chat_state::ChatStateEvent::PromptIndexChanged { .. }) |
                    Some(chat_state::ChatStateEvent::ContextPressureUpdated { .. }) => {
                        // Prompt index and token updates are informational —
                        // consumers query the actor directly when they need them.
                    }
                    None => {
                        // Actor shut down — no more events.
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
                            if session.state.lock().await.foreground.regular().is_some() {
                                session.drain_deferred_completions().await;
                            } else {
                                SessionActor::maybe_drain_notifications(
                                    session.clone(),
                                    completion_tx.clone(),
                                )
                                .await;
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
                    }
                }
            }
            maybe_completion = completion_rx.recv() => {
                let Some((prompt_id, result)) = maybe_completion else {
                    // Channel closed.
                    stop_permission_manager_and_drain_audit(&session).await;
                    shutdown_workflows(&session).await;
                    if !session.startup_hints.is_subagent {
                        session.checkpoint_running_task_notifications().await;
                    }
                    final_session_persistence_flush(&session).await;
                    cleanup_session_scratch(&session);
                    return;
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
                    state
                        .foreground
                        .regular()
                        .filter(|task| task.prompt_id == prompt_id)
                        .map(|task| task.origin.clone())
                };
                let (_turn_succeeded, suppress_goal_continuation, infra_pause_message) =
                    SessionActor::post_turn_goal_degradation_plan(
                        &result,
                        completed_origin.as_ref(),
                    );
                session.handle_completion(prompt_id, result).await;
                if let Some(message) = infra_pause_message {
                    session.apply_infra_pause_after_turn_err(message).await;
                }
                if !maybe_start_pending_manual_compaction(
                    session.clone(),
                    completion_tx.clone(),
                )
                .await
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
                        .handle_turn_end(suppress_goal_continuation)
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
                    tokio::task::spawn_local(async move {
                        s.maybe_fire_laziness_check().await;
                    });
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else {
                    stop_permission_manager_and_drain_audit(&session).await;
                    // ── session_end hook (channel-closed path) ────
                    // Fires BEFORE memory auto-save per plan contract.
                    let envelope = session.fire_hook(
                        ::hooks::event::HookEventName::SessionEnd,
                        None,
                        ::hooks::event::HookPayload::SessionEnd {
                            reason: "channel_closed".to_string(),
                            turn_count: None,
                            tool_call_count: None,
                        },
                    );
                    if let Some(registry) = session.hook_registry.borrow().clone() {
                        let ctx = session.hook_run_ctx();
                        let results = ::hooks::dispatcher::dispatch_non_blocking(
                            &registry,
                            ::hooks::event::HookEventName::SessionEnd,
                            &envelope,
                            &ctx,
                        )
                        .await;
                        session.send_hook_execution("session_end", None, None, &results).await;
                    }
                    session.dispatch_session_end_stop("channel_closed").await;
                    // Channel closed -- run memory session-end hook.
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
                                crate::session::memory::hooks::SessionEndResult::Written(_) => {
                                    "written"
                                }
                                crate::session::memory::hooks::SessionEndResult::Skipped => "skipped",
                                crate::session::memory::hooks::SessionEndResult::Failed(_) => "failed",
                            };
                            total_chunks_at_end = storage.total_chunk_count();
                            let telem = session.memory.diagnostics_snapshot();
                            tracing::info!(
                                target: ::diagnostics::memory_log::TARGET,
                                result = ?result,
                                tool_searches = telem.tool_search_count,
                                injection_searches = telem.injection_count,
                                recovery_searches = telem.compaction_recovery_count,
                                "MEMORY_SESSION_END: channel closed, session summary saved"
                            );
                            if let crate::session::memory::hooks::SessionEndResult::Written(
                                ref path_str,
                            ) = result
                            {
                                session.reindex_and_embed(std::path::Path::new(path_str), "session").await;
                                session.send_grow_notification(GrowSessionUpdate::MemorySessionSaved {
                                    path: path_str.clone(),
                                }).await;
                            }
                        }
                    } else {
                        tracing::debug!(
                            target: ::diagnostics::memory_log::TARGET,
                            "MEMORY_SUBAGENT_SKIP: skipping on_session_end for subagent session"
                        );
                    }
                    // Dream: attempt consolidation at session end
                    session.maybe_run_dream().await;
                    // Structured diagnostics after dream so counters are populated
                    let telem = session.memory.diagnostics_snapshot();
                    session.emit_memory_session_summary(&telem, total_chunks_at_end, session_end_result);
                    if let Some(notification) = replay_buffer.flush() {
                        session.emit_buffered(notification).await;
                    }
                    {
                        let model_id = session.current_model_id().await;
                        if let Some(signals) = session.signals_handle().snapshot().await {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::SessionEnded {
                                    duration_secs: session.session_start.elapsed().as_secs(),
                                    turn_count: signals.turn_count as u64,
                                    tool_call_count: signals.tool_call_count as u64,
                                    compaction_count: signals.compaction_count as u64,
                                    model_id,
                                },
                            );
                        }
                    }
                    shutdown_workflows(&session).await;
                    if !session.startup_hints.is_subagent {
                        session.checkpoint_running_task_notifications().await;
                    }
                    final_session_persistence_flush(&session).await;
                    session.signals_handle.shutdown();
                    cleanup_session_scratch(&session);
                    return;
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
                    SessionCommand::RestorePlanApproval => {
                        // Resume re-park: spawn the approval
                        // round-trip so the command loop is not blocked on
                        // the (open-ended) user decision.
                        //
                        // Detaching the handle is safe: the task is spawned on
                        // this session's `LocalSet`, so it is dropped (its
                        // `request_plan_approval` future cancelled, clearing
                        // `awaiting` via the guard) when the session ends — it
                        // cannot outlive the actor. `resume_plan_approval`
                        // also self-guards against a concurrent/duplicate
                        // re-park via the `pending_interactions` registry.
                        let s = session.clone();
                        let completion_tx = completion_tx.clone();
                        tokio::task::spawn_local(async move {
                            s.resume_plan_approval(completion_tx).await;
                        });
                    }
                    SessionCommand::QueuePrompt { prompt_id, prompt_blocks, origin, turn_kind, client_identifier, screen_mode, verbatim, json_schema, respond_to, persist_ack } => {
                        if let Err(error) = session.ensure_prefix_ready().await {
                            let _ = respond_to.send(Err(acp::Error::internal_error().data(
                                format!("session context was not durably published: {error}"),
                            )));
                            continue;
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
                        if !maybe_start_pending_manual_compaction(
                            session.clone(),
                            completion_tx.clone(),
                        )
                        .await
                        {
                            SessionActor::maybe_start_running_task(
                                session.clone(),
                                completion_tx.clone(),
                            )
                            .await;
                        }
                    }
                    SessionCommand::ExecuteSlashCommand { command, respond_to } => {
                        let result = session
                            .execute_out_of_band_slash_command(command)
                            .await;
                        if let Ok(Some(trigger)) = result.as_ref() {
                            // Preserve steering that reached the interjection
                            // buffer before the control invalidated this turn.
                            session
                                .cancel_turn_for_goal_control(trigger, &mut replay_buffer)
                                .await;
                        }
                        if !maybe_start_pending_manual_compaction(
                            session.clone(),
                            completion_tx.clone(),
                        )
                        .await
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
                            chat_state::NotificationSource::SubagentCompleted { subagent_id } => {
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
                    SessionCommand::BehaviorChange { session_mode, responds_to } => {
                        let outcome = session.request_behavior_change(session_mode).await;
                        let _ = outcome;
                        SessionActor::maybe_start_running_task(
                            session.clone(),
                            completion_tx.clone(),
                        )
                        .await;
                        let _ = responds_to.send(outcome);
                    }
                    SessionCommand::SetSessionModel { model_id, sampling_config, auto_compact_threshold_percent, responds_to } => {
                        let updated_model_id = session.handle_set_session_model(model_id, sampling_config, auto_compact_threshold_percent).await;
                        let _ = responds_to.send(updated_model_id);
                    }
                    SessionCommand::ReloadModelConfig {
                        model_id,
                        sampling_config,
                        image_description_model,
                        inference_idle_timeout,
                        max_retries,
                        auto_compact_threshold_percent,
                        responds_to,
                    } => {
                        let result = session
                            .handle_reload_model_config(
                                model_id,
                                sampling_config,
                                image_description_model,
                                inference_idle_timeout,
                                max_retries,
                                auto_compact_threshold_percent,
                            )
                            .await;
                        let _ = responds_to.send(result);
                    }
                    SessionCommand::RebuildAgentForDefinition { definition, responds_to } => {
                        let outcome = session.handle_rebuild_agent_for_definition(definition).await;
                        let _ = responds_to.send(outcome);
                    }
                    SessionCommand::OverrideModelName { model_name, extra_headers, context_window } => {
                        // Update the actor's SamplingConfig model + headers + context window.
                        if let Some(mut cfg) = session.chat_state_handle.get_sampling_config().await {
                            tracing::info!(
                                target: SESSION_LOG,
                                session_id = %session.session_info.id,
                                old_model = %cfg.model,
                                new_model = %model_name,
                                extra_header_count = extra_headers.len(),
                                old_context_window = cfg.context_window.get(),
                                new_context_window = ?context_window.map(|cw| cw.get()),
                                "OVERRIDE_MODEL: changing model name in sampling config"
                            );
                            // Update signals so primaryModelId and modelsUsed
                            // reflect the model used after the override, not
                            // the agent-level default (e.g. "grow-4.5").
                            // set_primary_model also adds to models_used.
                            session.signals_handle().set_primary_model(&model_name);
                            cfg.model = model_name.clone();
                            cfg.extra_headers.extend(extra_headers);
                            if let Some(cw) = context_window
                                && session.compaction.context_window_override.is_none()
                            {
                                cfg.context_window = cw;
                            }
                            session.chat_state_handle.update_sampling_config(cfg);

                            let existing = session.chat_state_handle.get_credentials().await;
                            if let Some(r) = crate::agent::config::try_resolve_model_credentials(model_name.as_str()) {
                                session.chat_state_handle.update_credentials(chat_state::Credentials {
                                    api_key: r.api_key,
                                    alpha_test_key: existing.alpha_test_key,
                                });
                            }
                            // Credentials changed under a possibly-unchanged model id.
                            session.invalidate_model_auth_memo();
                        }
                    }
                    SessionCommand::GetCurrentModel { responds_to } => {
                        let model = session.chat_state_handle.get_sampling_config().await
                            .map(|c| c.model)
                            .unwrap_or_default();
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

                        let hooks = match &*session.hook_registry.borrow() {
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
                            load_errors: session.hook_load_errors.borrow().clone(),
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
                        let _ = respond_to.send(session.plugin_registry.borrow().clone());
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
                    SessionCommand::RecordGoalTurnTaskIds { task_ids } => {
                        session.record_reparented_goal_turn_task_ids(task_ids);
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
                        session
                            .cancel_running_task(
                                cancel_subagents,
                                kill_background_tasks,
                                rewind_if_pristine,
                                trigger,
                            )
                            .await;

                        // Auto-pause the active Goal ONLY on an explicit
                        // user "Pause goal" intent (the Goal interrupt
                        // panel's choice carries `pause_goal: true`). Every
                        // other cancel — plain Esc/Ctrl+C outside Goal,
                        // StopTurnOnly / StopTurnAndSubagents, subagent
                        // teardown, lifecycle shutdown — leaves an active
                        // Goal untouched (it stays Active and may be
                        // continued by the next user input).
                        session.maybe_auto_pause_goal_on_cancel(pause_goal).await;

                        // Manual compaction admitted during the cancelled
                        // turn owns the next foreground slot; otherwise
                        // promote the queued prompt normally.
                        if !maybe_start_pending_manual_compaction(
                            session.clone(),
                            completion_tx.clone(),
                        )
                        .await
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
                            .admit_manual_compaction(
                                user_context,
                                completion_tx.clone(),
                                Some(respond_to),
                            )
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
                        tokio::task::spawn_local(async move {
                            let cwd = s.tool_context.cwd.as_path().to_string_lossy();
                            let skills_config = crate::util::config::load_config().await.skills;
                            let pr = s.plugin_registry.borrow().clone();
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
                        tokio::task::spawn_local(async move {
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
                            tracing::error!(
                                session_id = %session.session_info.id,
                                "rewind transaction is incomplete; stopping the actor for recovery"
                            );
                            break;
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
                        tokio::task::spawn_local(async move {
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
                        tokio::task::spawn_local(async move {
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
                        tokio::task::spawn_local(async move {
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
                        let _ = respond_to.send(session.client_hooks.borrow().clone());
                    }
                    SessionCommand::SetClientHooks { hooks } => {
                        *session.client_hooks.borrow_mut() = hooks;
                    }
                    SessionCommand::GetMcpStatus { respond_to } => {
                        let mcp_state = session.mcp_state.clone();
                        let tool_bridge = session.agent.borrow().tool_bridge().clone();
                        tokio::task::spawn_local(async move {
                            let snapshot = crate::extensions::mcp::build_mcp_status(
                                &mcp_state,
                                &tool_bridge,
                            ).await;
                            let _ = respond_to.send(snapshot);
                        });
                    }
                    SessionCommand::CallMcpTool { server_name, server_url, tool_name, arguments, respond_to } => {
                        let mcp_state = session.mcp_state.clone();
                        tokio::task::spawn_local(async move {
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
                        tokio::task::spawn_local(async move {
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
                        tokio::task::spawn_local(async move {
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
                        if let Some(registry) = session.hook_registry.borrow().clone() {
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
                        tokio::task::spawn_local(async move {
                            let result = s.handle_side_question(&question).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::Recap { auto } => {
                        let s = session.clone();
                        tokio::task::spawn_local(async move {
                            s.handle_recap(auto).await;
                        });
                    }
                    SessionCommand::AISuggest { prefix, cwd, model_override, respond_to } => {
                        let s = session.clone();
                        tokio::task::spawn_local(async move {
                            let result = s.handle_ai_suggest(&prefix, &cwd, model_override.as_deref()).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::SuggestPrompt { model_override, respond_to } => {
                        let s = session.clone();
                        tokio::task::spawn_local(async move {
                            let result = s.handle_suggest_prompt(model_override.as_deref()).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::RewriteMemoryNote { raw_text, context_summary, respond_to } => {
                        let s = session.clone();
                        tokio::task::spawn_local(async move {
                            let result = s.handle_rewrite_memory_note(&raw_text, &context_summary).await;
                            let _ = respond_to.send(result);
                        });
                    }
                    SessionCommand::SteerTurn { expected_turn_id, text, id, images, respond_to } => {
                        let admitted = {
                            let state = session.state.lock().await;
                            state.foreground.regular().is_some_and(|task| {
                                task.prompt_id == expected_turn_id && !task.is_finished()
                            })
                        };
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
                            image_count: images.len() as u32,
                            redirect_kind: crate::session::events::RedirectKind::Interjection,
                        });
                        session.queue_mid_turn_interjection(text, images);
                        let _ = respond_to.send(Ok(()));
                        tracing::info!(expected_turn_id, "Queued same-turn steering input");
                    }
                    SessionCommand::WorkflowCompleted { state, outcome } => {
                        let run_id = state.run_id.clone();
                        if session.behavior.lock().deep_research_run_id() == Some(&run_id) {
                            session.finish_deep_research_run(&run_id, outcome).await;
                            continue;
                        }
                        session.send_available_commands_update().await;
                        let revision = state.revision;
                        let prompt_text = session
                            .workflow_completion_notification(&state)
                            .await;
                        if let Err(error) = session
                            .receive_notification(
                                chat_state::NotificationSource::WorkflowCompleted {
                                    run_id: run_id.clone(),
                                },
                                chat_state::NotificationSourceVersion::Ordinal { value: revision },
                                prompt_text.to_owned(),
                            )
                            .await
                        {
                            tracing::error!(run_id, revision, %error, "workflow notification admission failed");
                            continue;
                        }
                        SessionActor::maybe_drain_notifications(
                            session.clone(),
                            completion_tx.clone(),
                        )
                        .await;
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
                        if let SessionCommand::UnloadIfIdle { respond_to } = command {
                            // This decision and mailbox close are performed in
                            // one actor turn. Commands already ahead of this one
                            // have been applied; commands behind it are ordered
                            // after the unload request and are rejected once the
                            // receiver is closed.
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
                            cmd_rx.close();
                            let _ = respond_to.send(true);
                        }
                        stop_permission_manager_and_drain_audit(&session).await;
                        shutdown_workflows(&session).await;
                        // Flush the actor-owned replay buffer so any
                        // streamed chunks still pending at shutdown
                        // (e.g. reasoning text from a sampler stream
                        // racing with a CLI exit / harness teardown)
                        // are committed to updates.jsonl before the
                        // local session directory is finalized. Mirrors the same flush in the
                        // Cancel, CopyFile, and FlushComplete arms.
                        if let Some(notification) = replay_buffer.flush() {
                            session.emit_buffered(notification).await;
                        }
                        // Drop any queued synthetic auto-wake prompts and pending
                        // notifications before running hooks. Without this, a
                        // synthetic prompt that slipped through the per-tool-result
                        // sweep could still be accepted into Timeline by a later
                        // path, producing a trailing
                        // `<system-reminder>` with no assistant reply. Placed
                        // BEFORE hook dispatch so the cleanup runs even if hooks
                        // abort.
                        session.drop_pending_synthetic_items().await;

                        // ── session_end hook (shutdown path) ────────
                        // Fires BEFORE memory auto-save per plan contract.
                        let envelope = session.fire_hook(
                            ::hooks::event::HookEventName::SessionEnd,
                            None,
                            ::hooks::event::HookPayload::SessionEnd {
                                reason: "shutdown".to_string(),
                                turn_count: None,
                                tool_call_count: None,
                            },
                        );
                        if let Some(registry) = session.hook_registry.borrow().clone() {
                            let ctx = session.hook_run_ctx();
                            let results = ::hooks::dispatcher::dispatch_non_blocking(
                                &registry,
                                ::hooks::event::HookEventName::SessionEnd,
                                &envelope,
                                &ctx,
                            )
                            .await;
                            session.send_hook_execution("session_end", None, None, &results).await;
                        }
                        session.dispatch_session_end_stop("shutdown").await;
                        // Memory: save session summary before shutdown
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
                                    crate::session::memory::hooks::SessionEndResult::Written(_) => {
                                        "written"
                                    }
                                    crate::session::memory::hooks::SessionEndResult::Skipped => {
                                        "skipped"
                                    }
                                    crate::session::memory::hooks::SessionEndResult::Failed(_) => {
                                        "failed"
                                    }
                                };
                                total_chunks_at_end = storage.total_chunk_count();
                                let telem = session.memory.diagnostics_snapshot();
                                tracing::info!(
                                    target: ::diagnostics::memory_log::TARGET,
                                    result = ?result,
                                    tool_searches = telem.tool_search_count,
                                    injection_searches = telem.injection_count,
                                    recovery_searches = telem.compaction_recovery_count,
                                    "MEMORY_SESSION_END: session summary saved"
                                );
                                // Reindex + embed the written file so it's searchable next session
                                if let crate::session::memory::hooks::SessionEndResult::Written(
                                    ref path_str,
                                ) = result
                                {
                                    session.reindex_and_embed(std::path::Path::new(path_str), "session").await;
                                    session.send_grow_notification(GrowSessionUpdate::MemorySessionSaved {
                                        path: path_str.clone(),
                                    }).await;
                                }
                            }
                        } else {
                            tracing::debug!(
                                target: ::diagnostics::memory_log::TARGET,
                                "MEMORY_SUBAGENT_SKIP: skipping on_session_end for subagent session"
                            );
                        }
                        // Dream: attempt consolidation at session end
                        session.maybe_run_dream().await;
                        // Structured diagnostics after dream so counters are populated
                        let telem = session.memory.diagnostics_snapshot();
                        session.emit_memory_session_summary(&telem, total_chunks_at_end, session_end_result);
                        if !session.startup_hints.is_subagent {
                            session.checkpoint_running_task_notifications().await;
                        }
                        final_session_persistence_flush(&session).await;
                        session.signals_handle.shutdown();
                        // Clean up scratch directory (pre-edit file copies).
                        cleanup_session_scratch(&session);
                        return;
                    }
                }
            }
            _ = session.idle_arbiter.notified() => {
                arbitrate_idle_wake(session.clone(), completion_tx.clone()).await;
            }
        }
    }
}
