//! Session teardown helpers.
use super::*;

/// Best-effort removal of this session's per-session scratch staging on
/// teardown. A no-op in builds without a scratch producer.
pub(super) fn cleanup_session_scratch(_session: &SessionActor) {}

/// Close sampler admission, cancel and join provider requests, then drain all
/// already-emitted sampler events before the final persistence frontier.
pub(super) async fn shutdown_sampler(session: &SessionActor) -> Result<(), String> {
    session.sampler_handle.close();
    let mut errors = Vec::new();
    if let Some(mut owner) = session.sampler_owner.borrow_mut().take()
        && let Err(error) = owner
            .shutdown_bounded(std::time::Duration::from_secs(10))
            .await
    {
        errors.push(error);
    }
    if let Some(mut drainer) = session.sampler_event_drainer.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut drainer).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => errors.push(format!("sampler event drainer failed: {error}")),
            Err(_) => {
                drainer.abort();
                let _ = drainer.await;
                errors.push("sampler event drainer timed out".into());
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod sampler_shutdown_tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_sampler_joins_drainer_and_breaks_session_cycle() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let mut actor = crate::session::actor::tests::support::create_test_actor(
                    0,
                    100_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                let owner = sampler::SamplerActor::spawn_owned(
                    sampler::SamplerConfig::default(),
                    sampler::RetryPolicy::default(),
                    event_tx,
                );
                actor.sampler_handle = owner.handle();
                *actor.sampler_owner.borrow_mut() = Some(owner);
                let actor = std::sync::Arc::new(actor);
                let weak_actor = std::sync::Arc::downgrade(&actor);
                let drainer_actor = actor.clone();
                actor
                    .sampler_event_drainer
                    .arm(tokio::task::spawn_local(async move {
                        while let Some(event) = event_rx.recv().await {
                            drainer_actor.handle_sampling_event(event).await;
                        }
                    }));
                assert_eq!(std::sync::Arc::strong_count(&actor), 2);

                shutdown_sampler(&actor)
                    .await
                    .expect("sampler teardown barrier");

                assert_eq!(std::sync::Arc::strong_count(&actor), 1);
                assert!(!actor.sampler_event_drainer.is_running());
                drop(actor);
                assert!(
                    weak_actor.upgrade().is_none(),
                    "sampler drainer must not retain the Session after teardown"
                );
            })
            .await;
    }
}

/// Close long-lived session service ingress and join their LocalSet owners.
/// Every owner is attempted even when an earlier one fails. The caller must
/// treat an error as a failed persistence barrier rather than logging and
/// continuing across the final Session frontier.
pub(super) async fn stop_session_background_services(
    session: &SessionActor,
    fatal: bool,
) -> Result<(), String> {
    let mut errors = Vec::new();
    session.background_service_shutdown.cancel();
    for (label, slot) in [
        ("ask-user-question service", &session.user_question_worker),
        ("context-recall service", &session.context_recall_worker),
    ] {
        if let Err(error) = join_background_service(slot, label).await {
            tracing::warn!(service = label, %error, "failed to join Session background service");
            errors.push(error);
        }
    }
    // These workers may own blocking filesystem/SQLite operations. Aborting
    // their async wrapper would detach the inner blocking job and let an old
    // actor mutate shared artifacts after a reload. Wait for exact ownership
    // transfer instead; the shared cancellation token stops work between
    // bounded batches/files.
    for (label, slot) in [
        (
            "notification payload reconciler",
            &session.notification_reconciliation_worker,
        ),
        ("initial memory reindex", &session.memory_reindex_worker),
    ] {
        if let Err(error) = join_background_service_exact(slot, label).await {
            tracing::warn!(service = label, %error, "Session storage worker failed while joining");
            errors.push(error);
        }
    }
    if let Some(extension) = &session.idle_prompt_extension
        && let Err(error) = extension.shutdown().await
    {
        tracing::warn!(%error, "failed to join idle notification timer");
        errors.push(format!("idle notification timer: {error}"));
    }
    let fs_watch = session.fs_watch_handle.borrow_mut().take();
    if let Some(fs_watch) = fs_watch
        && let Err(error) = fs_watch.shutdown_and_join().await
    {
        tracing::warn!(%error, "failed to join filesystem watcher");
        errors.push(format!("filesystem watcher: {error}"));
    }
    if let Err(error) = session.mcp_initialization_worker.abort_and_join().await {
        tracing::warn!(%error, "failed to join MCP initialization worker");
        errors.push(format!("MCP initialization worker: {error}"));
    }
    session.mcp_state.lock().await.set_client_event_tx(None);
    if let Err(error) = session.project_discovery_worker.abort_and_join().await {
        tracing::warn!(%error, "failed to join project discovery watcher");
        errors.push(format!("project discovery watcher: {error}"));
    }
    if let Some(mut dispatcher) = session.mcp_dispatcher_worker.take() {
        if fatal {
            dispatcher.abort();
        }
        match tokio::time::timeout(std::time::Duration::from_secs(3), &mut dispatcher).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) if fatal && error.is_cancelled() => {}
            Ok(Err(error)) => {
                errors.push(format!("MCP status dispatcher failed: {error}"));
            }
            Err(_) => {
                tracing::warn!("MCP status dispatcher did not stop cooperatively; aborting");
                dispatcher.abort();
                let _ = dispatcher.await;
                errors.push("MCP status dispatcher did not stop within 3 seconds".to_string());
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn join_background_service(slot: &TaskSlot<()>, label: &'static str) -> Result<(), String> {
    let Some(mut worker) = slot.take() else {
        return Ok(());
    };
    match tokio::time::timeout(std::time::Duration::from_secs(10), &mut worker).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("{label} failed: {error}")),
        Err(_) => {
            worker.abort();
            let _ = worker.await;
            Err(format!("{label} did not stop within 10 seconds"))
        }
    }
}

async fn join_background_service_exact(
    slot: &TaskSlot<()>,
    label: &'static str,
) -> Result<(), String> {
    let Some(worker) = slot.take() else {
        return Ok(());
    };
    worker
        .await
        .map_err(|error| format!("{label} failed: {error}"))
}

pub(super) async fn shutdown_workflows(session: &SessionActor) -> Result<(), Vec<String>> {
    // Workflow ingress is a Session-owned worker. Close the manager gate
    // first, then wake and join the worker so queued envelopes are explicitly
    // rejected before executor cancellation begins. This ordering prevents a
    // drain from racing a late workspace mutation or Run admission.
    {
        let mut manager = session.workflow_manager.lock().await;
        manager.close_admission();
    }
    session.workflow_service_shutdown.cancel();
    if let Some(mut worker) = session.workflow_worker.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut worker).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!(%error, "Workflow ingress worker failed during session teardown");
                return Err(vec![format!("workflow ingress worker: {error}")]);
            }
            Err(error) => {
                tracing::error!(%error, "Workflow ingress worker did not stop during session teardown");
                worker.abort();
                let _ = worker.await;
                return Err(vec![format!("workflow ingress worker: {error}")]);
            }
        }
    }
    let drain = session
        .workflow_manager
        .lock()
        .await
        .cancel_all_and_drain(std::time::Duration::from_secs(30))
        .await;
    if let Err(run_ids) = &drain {
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
        return drain;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            tracing::warn!("workflow shutdown persistence actor dropped flush ack")
        }
        Err(_) => tracing::warn!("workflow shutdown persistence flush timed out"),
    }
    drain
}

/// Close the primary-owned permission authority at teardown entry and wait
/// until its final audit event has crossed the bridge. End hooks and memory
/// work may take time; no child permission request remains live while they run.
pub(super) async fn stop_permission_manager_and_drain_audit(
    session: &SessionActor,
) -> Result<(), String> {
    if session.owns_permission_manager {
        session.permissions.shutdown_and_drain().await;
        let bridge = session.permission_audit_bridge.lock().take();
        if let Some(mut bridge) = bridge {
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut bridge).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => return Err(error),
                Ok(Err(error)) => {
                    return Err(format!(
                        "permission audit bridge failed during session shutdown: {error}"
                    ));
                }
                Err(_) => {
                    // Exact UI append retries are deliberately unbounded
                    // during normal operation. Shutdown bounds the bridge;
                    // cancelling its dedicated writer epoch makes the retry
                    // resolve, and the caller withholds the final frontier.
                    session.durable_ui_cancel.cancel();
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(2), &mut bridge).await;
                    bridge.abort();
                    let _ = bridge.await;
                    return Err(
                        "permission audit bridge did not drain within 5 seconds".to_string()
                    );
                }
            }
        }
    }
    Ok(())
}

/// Cross the final persistence barrier after every teardown producer,
/// including the drained permission bridge and session-end hooks, has stopped.
pub(super) async fn final_session_persistence_flush(session: &SessionActor) {
    // Every session-end producer (hooks, memory dream, Workflow/Goal
    // checkpoints and permission audit) has stopped at this boundary. Exact
    // Sideband retries remain live until now so teardown work can still use
    // the same durable auxiliary-model protocol.
    session.fail_stop_sideband_admission().await;
    session.sideband_cancel.cancel();
    session.finalizer_sideband_cancel.cancel();
    session.sideband_repair_cancel.cancel();
    session.durable_ui_cancel.cancel();
    if tokio::time::timeout(
        std::time::Duration::from_secs(5),
        session.session_activities.wait_idle(),
    )
    .await
    .is_err()
    {
        // Do not declare a final durable frontier while an owner can still
        // append behind it. Closing the persistence sender when the Session
        // drops is safer than publishing a false final-flush guarantee.
        tracing::error!("final persistence flush withheld because session writers did not drain");
        return;
    }
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
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => tracing::warn!("final persistence actor dropped flush ack"),
        Err(_) => tracing::warn!("final persistence flush timed out"),
    }
}

/// Stop a session whose Timeline writer epoch has permanently failed. No
/// session-end hook or memory mutation is admitted after causal persistence is
/// unavailable; only local authorities and child execution are unwound.
pub(super) async fn terminate_failed_timeline_writer(
    session: &SessionActor,
    cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
) {
    // Fatal teardown is a fail-stop barrier, not merely a final flush. Close
    // admission and cancel every detached owner before dismantling shared
    // authorities so no producer can append UI/persistence work afterwards.
    let _ = latch_termination_and_cancel_controls(session, TerminationState::Fatal).await;
    let (regular_task, removed_inputs) = {
        let mut state = session.state.lock().await;
        state.pending_manual_compact = None;
        state.applying_step_control = None;
        state.applying_behavior_control = None;
        state.behavior_control_worker_active = false;
        state.behavior_control_foreground_claimed = false;
        let regular_task = state.foreground.take_regular();
        state.foreground = ForegroundState::Idle;
        let removed_inputs = std::mem::take(&mut state.pending_inputs);
        session.broadcast_queue_changed(&state);
        (regular_task, removed_inputs)
    };
    session.session_activities.close_admission();
    session.fail_stop_sideband_admission().await;
    session
        .user_input_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let background_services_drained = match stop_session_background_services(session, true).await {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, "fatal teardown failed to drain Session background services");
            false
        }
    };
    for input in removed_inputs {
        SessionActor::respond_removed_prompt(input.respond_to);
    }
    if let Some(task) = regular_task.as_ref() {
        task.handle.abort();
    }
    for handle in [
        session.step_control_worker.take(),
        session.behavior_control_worker.take(),
    ]
    .into_iter()
    .flatten()
    {
        handle.abort();
        let _ = handle.await;
    }
    if let Err(error) = session.goal_drive.abort_and_join().await {
        tracing::warn!(%error, "fatal teardown failed to join Goal drive");
    }
    if let Err(error) = session.deferred_prefix.abort_and_join().await {
        tracing::warn!(%error, "fatal teardown failed to join prefix preparation");
    }
    if let Err(error) = session.restored_plan_approval.abort_and_join().await {
        tracing::warn!(%error, "fatal teardown failed to join restored Plan approval");
    }
    session.compaction.cancel.request_cancel();
    session.sideband_cancel.cancel();
    session.finalizer_sideband_cancel.cancel();
    session.sideband_repair_cancel.cancel();
    let subagents_drained = match cancel_and_drain_session_subagents(session, cmd_rx).await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "fatal teardown timed out draining non-Workflow subagents");
            false
        }
    };
    cmd_rx.close();
    let sampler_drained = match shutdown_sampler(session).await {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, "fatal sampler shutdown failed");
            false
        }
    };

    // Abort is cooperative at the async scheduling boundary. Wait briefly for
    // the regular runner, manual compaction lease and detached memory owners
    // to release their guards before the final persistence flush.
    let owners_drained = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let regular_done = regular_task.as_ref().is_none_or(AgentTask::is_finished);
            let compaction_done = !session.compaction.lease.is_in_flight();
            let memory_done = !session
                .memory
                .is_flushing
                .load(std::sync::atomic::Ordering::Acquire)
                && !session
                    .memory
                    .is_dreaming
                    .load(std::sync::atomic::Ordering::Acquire);
            let detached_done = session.session_activities.is_idle();
            if regular_done && compaction_done && memory_done && detached_done {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();
    if !owners_drained || !subagents_drained || !background_services_drained || !sampler_drained {
        tracing::error!(
            owners_drained,
            subagents_drained,
            background_services_drained,
            sampler_drained,
            "fatal teardown timed out while draining session owners; withholding all final persistence"
        );
        // Do not reacquire Goal, Workflow, permission, or persistence locks
        // that may still be held by the undrained owner. Revoke Workflow
        // execution opportunistically without waiting; dropping the Session
        // remains the only safe fail-stop boundary in this state.
        if let Ok(workflows) = session.workflow_manager.try_lock() {
            workflows.request_cancel_all();
        } else {
            tracing::error!(
                "fatal teardown could not acquire Workflow cancellation authority without blocking"
            );
        }
        session.durable_ui_cancel.cancel();
        session.signals_handle.shutdown();
        cleanup_session_scratch(session);
        return;
    }
    let permission_audit_error = stop_permission_manager_and_drain_audit(session).await.err();
    if let Err(run_ids) = shutdown_workflows(session).await {
        tracing::error!(
            ?run_ids,
            "fatal Workflow owners did not drain; withholding Goal close and final persistence frontier"
        );
        session.durable_ui_cancel.cancel();
        session.signals_handle.shutdown();
        cleanup_session_scratch(session);
        return;
    }
    // Fatal teardown must close the same Goal accounting window as graceful
    // shutdown. A child or Sideband may have received provider usage before
    // the owner that was publishing its terminal failed. If the Timeline is
    // still healthy, persist that exact usage (or the fail-closed incomplete
    // marker) before the final frontier. If persistence itself is broken,
    // never publish a misleading final flush over the stale Goal snapshot.
    match session.settle_goal_usage_for_shutdown().await {
        Ok(()) => {
            session.checkpoint_goal_before_shutdown().await;
            if let Some(error) = permission_audit_error {
                tracing::error!(
                    %error,
                    "fatal permission audit drain failed; withholding final persistence frontier"
                );
                session.durable_ui_cancel.cancel();
            } else {
                final_session_persistence_flush(session).await;
            }
        }
        Err(error) => {
            tracing::error!(
                %error,
                "fatal Goal usage settlement failed; withholding final persistence frontier"
            );
            session.fail_stop_sideband_admission().await;
            session.sideband_cancel.cancel();
            session.finalizer_sideband_cancel.cancel();
            session.sideband_repair_cancel.cancel();
            session.durable_ui_cancel.cancel();
        }
    }
    session.signals_handle.shutdown();
    cleanup_session_scratch(session);
}
