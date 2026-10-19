//! Persistence-only continuation of a child that has already stopped executing.
use super::*;
use crate::session::storage::StorageAdapter;

async fn run_owned_blocking<T>(
    in_flight: Arc<std::sync::atomic::AtomicBool>,
    deadline: tokio::time::Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    run_owned_blocking_inner(in_flight, deadline, work, None).await
}

async fn run_owned_blocking_inner<T>(
    in_flight: Arc<std::sync::atomic::AtomicBool>,
    deadline: tokio::time::Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    worker_exit: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<T, String>
where
    T: Send + 'static,
{
    use std::sync::atomic::Ordering;

    if tokio::time::Instant::now() >= deadline {
        return Err("finalization deadline elapsed before recovery I/O".into());
    }
    if in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("another child publication or recovery I/O still owns the session".into());
    }
    let worker_owner = in_flight.clone();
    let worker = tokio::task::spawn_blocking(move || {
        struct BlockingOwner {
            in_flight: Arc<std::sync::atomic::AtomicBool>,
            worker_exit: Option<tokio::sync::oneshot::Sender<()>>,
        }
        impl Drop for BlockingOwner {
            fn drop(&mut self) {
                self.in_flight.store(false, Ordering::Release);
                if let Some(worker_exit) = self.worker_exit.take() {
                    let _ = worker_exit.send(());
                }
            }
        }
        let _owner = BlockingOwner {
            in_flight: worker_owner,
            worker_exit,
        };
        if tokio::time::Instant::now() >= deadline {
            return Err("finalization deadline elapsed before recovery I/O".into());
        }
        work()
    });
    tokio::time::timeout_at(deadline, worker)
        .await
        .map_err(|_| "finalization deadline elapsed; recovery worker retains ownership".to_owned())?
        .map_err(|error| format!("child recovery worker failed: {error}"))?
}

pub(crate) fn refresh_finalization_parent(
    data: &mut ShellCompletionData,
    parent: &crate::session::SessionHandle,
    completion_parent: Option<&crate::session::SessionHandle>,
    gate: Arc<crate::session::storage_admission::StorageAdmission>,
) {
    if !parent.chat_state_handle.is_closed() {
        let epoch = parent.runtime_observation.read().control_epoch.clone();
        if data
            .parent_epoch
            .as_ref()
            .is_some_and(|previous| previous != &epoch)
        {
            // A healthy replacement can only be published after the former
            // actor and persistence owner drain. An absent receipt is now
            // proof that the old mailbox operation cannot still execute.
            data.receipt_in_flight = false;
            data.usage_in_flight = false;
        }
        data.parent_epoch = Some(epoch);
        data.parent_chat_state = Some(parent.chat_state_handle.clone());
        data.storage_admission = Some(gate);
        data.parent_cmd_tx = Some(parent.cmd_tx.clone());
        let completion_parent =
            completion_parent.filter(|handle| !handle.chat_state_handle.is_closed());
        let completion_epoch =
            completion_parent.map(|handle| handle.runtime_observation.read().control_epoch.clone());
        if data
            .completion_parent_epoch
            .as_ref()
            .is_some_and(|previous| Some(previous) != completion_epoch.as_ref())
        {
            data.receipt_in_flight = false;
        }
        data.completion_parent_epoch = completion_epoch;
        data.completion_cmd_tx = completion_parent.map(|handle| handle.cmd_tx.clone());
    }
}

pub(crate) async fn reconcile_shell_child(
    request: SubagentRequest,
    mut output: ChildRunOutput<ShellCompletionData>,
    deadline: tokio::time::Instant,
) -> ChildRunOutput<ShellCompletionData> {
    output.completion_data.finalization_deadline = Some(deadline);
    let result = tokio::time::timeout_at(deadline, advance(&request, &mut output))
        .await
        .unwrap_or_else(|_| {
            Err("finalization period deadline reached; dispatched IO retains its lease".into())
        });
    match result {
        Ok(()) => {
            output.result.finalization_pending = !output.completion_data.terminal_committed();
            output.completion_data.finalization_error = None;
        }
        Err(error) => {
            output.result.finalization_pending = true;
            if output.completion_data.finalization_error.is_none() {
                output.completion_data.finalization_error = Some(FinalizationFailure::observation(
                    FinalizationStage::RecoveryOwnership,
                    error.clone(),
                ));
            }
            tracing::warn!(subagent_id = %request.id, %error, "child finalization remains owned and pending");
        }
    }
    output
}

async fn advance(
    request: &SubagentRequest,
    output: &mut ChildRunOutput<ShellCompletionData>,
) -> Result<(), String> {
    let data = &mut output.completion_data;
    let parent = data
        .parent_chat_state
        .clone()
        .ok_or("parent writer unavailable")?;
    if parent.is_closed() {
        return Err("parent writer has failed; a fully drained replacement is required".into());
    }
    if data
        .output_write_in_flight
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(
            "original output publication remains in flight; no replacement writer is acquired"
                .into(),
        );
    }
    // No new child writer while either the actor or persistence rail remains.
    if data
        .child_thread
        .as_ref()
        .is_some_and(|thread| !thread.is_finished())
    {
        return Err("previous child actor/persistence owner has not drained".into());
    }
    let events = tokio::time::timeout(std::time::Duration::from_secs(5), parent.timeline_events())
        .await
        .map_err(|_| "parent prefix observation timed out")?
        .ok_or("parent prefix observation unavailable")?;
    let timeline = chat_state::Timeline::from_events(events).map_err(|e| e.to_string())?;
    if data.usage_in_flight && timeline.events().iter().any(|event| {
        matches!(&event.kind, chat_state::TimelineEventKind::Observation(observation)
            if observation.scope == "session_usage" && observation.name == "subagent_settled"
                && observation.data.as_ref().and_then(|data| data.get("subagent_id")).and_then(serde_json::Value::as_str) == Some(request.id.as_str()))
    }) {
        data.usage_applied = true;
        data.usage_in_flight = false;
    }
    let (spawn_seq, spawn) =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == request.id => Some((event.seq, spawn.clone())),
                _ => None,
            })
            .ok_or("original parent spawn is not confirmed")?;
    if spawn.child_session_id != output.result.child_session_id {
        return Err("child finalization identity conflict".into());
    }

    if !data.terminal_committed {
        let deadline = data
            .finalization_deadline
            .expect("finalization cycle deadline");
        let child_session_id = spawn.child_session_id.clone();
        let parent_session_id = request.parent_session_id.clone();
        let subagent_id = request.id.clone();
        let output_unavailable = output.result.output_unavailable;
        let has_persisted_output_ref = data.persisted_output_ref.is_some();
        let spawn_for_worker = spawn.clone();
        let fallback = RecoveredInspectionResult {
            event: subagent_result_fact(&output.result, data.persisted_output_ref.clone()),
            output: Some(output.result.output.to_string()),
        };
        let pending_child_event = data
            .child_chat_state
            .as_ref()
            .and_then(|handle| handle.pending_timeline_commit())
            .map(|pending| pending.event);
        let storage_admission = data.storage_admission.clone();
        let worker_result = run_owned_blocking(
            data.output_write_in_flight.clone(),
            deadline,
            move || {
                let runtime = tokio::runtime::Handle::current();
                runtime.block_on(async move {
                    if tokio::time::Instant::now() >= deadline {
                        return Err("finalization deadline elapsed before child recovery".into());
                    }
                    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
                    let opened = storage
                        .open_session_by_id_shared_read(&child_session_id)
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| "child recovery entity unavailable".to_owned())?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err("finalization deadline elapsed after child open".into());
                    }
                    let recovery_token = if let Some(gate) = storage_admission {
                        gate.prepare_recovery_before(&opened.summary().info, deadline)
                            .await
                            .map_err(|error| error.to_string())?
                    } else {
                        None
                    };
                    storage
                        .claim_recovery_writer(&opened.summary().info)
                        .map_err(|error| error.to_string())?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err("finalization deadline elapsed after child writer claim".into());
                    }
                    if let Some(event) = pending_child_event {
                        let append = storage.append_timeline_event_durable(
                            &opened.summary().info,
                            &event,
                        );
                        tokio::time::timeout_at(deadline, append)
                            .await
                            .map_err(|_| {
                                "finalization deadline elapsed waiting for exact pending child event"
                                    .to_owned()
                            })?
                            .map_err(|error| format!("exact pending child event: {error}"))?;
                    }
                    let durable_child = opened
                        .validated_timeline(&child_session_id)
                        .map_err(|error| error.to_string())?
                        .timeline;
                    if tokio::time::Instant::now() >= deadline {
                        return Err("finalization deadline elapsed after child validation".into());
                    }
                    let canonical_result_exists = durable_child.events().iter().any(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::SubagentResult(_)
                        )
                    });
                    if output_unavailable
                        && !has_persisted_output_ref
                        && !canonical_result_exists
                    {
                        return Err("original output exceeds retention budget or is unavailable; refusing to invent replacement content".into());
                    }
                    let (reference, result, recovered_output) =
                        ensure_recovered_child_result_with_opened(
                            &parent_session_id,
                            spawn_seq,
                            &spawn_for_worker,
                            fallback,
                            &opened,
                            &storage,
                            Some(deadline),
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err(format!("finalization deadline elapsed after child result recovery for {subagent_id}"));
                    }
                    let child_timeline = opened
                        .validated_timeline(&child_session_id)
                        .map_err(|error| error.to_string())?
                        .timeline;
                    if tokio::time::Instant::now() >= deadline {
                        return Err("finalization deadline elapsed after child usage validation".into());
                    }
                    let usage = chat_state::actor::state::project_timeline_usage(&child_timeline)
                        .map_err(|error| error.to_string())?;
                    Ok((
                        storage,
                        opened,
                        recovery_token,
                        child_timeline,
                        reference,
                        result,
                        recovered_output,
                        usage.by_model.into_iter().collect::<Vec<_>>(),
                        usage.incomplete,
                    ))
                })
            },
        )
        .await?;
        let (
            storage,
            opened,
            recovery_token,
            child_timeline,
            reference,
            result,
            recovered_output,
            child_usage_by_model,
            child_usage_incomplete,
        ) = worker_result;
        // A durable result may predate a lost ACK. It must match the original
        // execution outcome; a later caller cannot overwrite it.
        if data.execution_outcome_confirmed && result.outcome != subagent_outcome(&output.result) {
            return Err("canonical child outcome conflicts with retained execution outcome".into());
        }
        data.child_result_ref = Some(reference.clone());
        data.persisted_output_ref = result.output_ref.clone();
        output.result.success = result.outcome == chat_state::SubagentOutcome::Completed;
        output.result.cancelled = result.outcome == chat_state::SubagentOutcome::Cancelled;
        output.result.error = result.error.clone();
        output.result.total_tokens_used = result.tokens_used;
        output.result.tokens_used = result.tokens_used;
        output.result.tool_calls = result.tool_calls;
        output.result.turns = result.turns;
        output.result.duration_ms = result.duration_ms;
        output.result.output_unavailable =
            result.output_ref.is_some() && recovered_output.is_none();
        // The same attempt/subagent IDs make settlement idempotent.
        if !data.usage_applied {
            if data.usage_in_flight {
                return Err("original parent usage settlement remains in flight; no duplicate mutation is dispatched".into());
            }
            data.usage_in_flight = true;
            data.usage_applied = handle_request::record_subagent_usage_before(
                data.parent_cmd_tx.as_ref(),
                request.id.clone(),
                Some(child_usage_by_model.clone()),
                request.parent_prompt_id.clone(),
                child_usage_incomplete || output.result.output_usage_incomplete,
                data.finalization_deadline,
            )
            .await;
            if !data.usage_applied {
                return Err("parent usage settlement remains unconfirmed".into());
            }
            data.usage_in_flight = false;
        }
        let existing = timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    terminal,
                )) if terminal.subagent_id == request.id => Some(terminal),
                _ => None,
            });
        if let Some(terminal) = existing {
            child_timeline
                .validate_subagent_result_link(
                    &request.parent_session_id,
                    spawn_seq,
                    &spawn,
                    terminal,
                )
                .map_err(|e| e.to_string())?;
        } else {
            record_parent_subagent_end_before(
                &parent,
                &output.result,
                Some(reference),
                data.snapshot_ref.clone(),
                data.finalization_deadline
                    .expect("finalization cycle deadline"),
            )
            .await
            .map_err(|error| {
                data.finalization_error = Some(FinalizationFailure::timeline(
                    FinalizationStage::ParentTerminal,
                    &error,
                    parent.pending_timeline_commit(),
                ));
                error.to_string()
            })?;
        }
        if let (Some(gate), Some(token)) = (&data.storage_admission, recovery_token) {
            let sync_storage = storage.clone();
            let sync_info = opened.summary().info.clone();
            let sync_deadline = data
                .finalization_deadline
                .expect("finalization cycle deadline");
            run_owned_blocking(
                data.output_write_in_flight.clone(),
                sync_deadline,
                move || {
                    tokio::runtime::Handle::current().block_on(async move {
                        let deadline = sync_deadline;
                        if tokio::time::Instant::now() >= deadline {
                            return Err(
                                "finalization deadline elapsed before recovery prefix sync".into(),
                            );
                        }
                        tokio::time::timeout_at(
                            deadline,
                            sync_storage.sync_recovery_prefix(&sync_info),
                        )
                        .await
                        .map_err(|_| {
                            "finalization deadline elapsed waiting for recovery prefix sync"
                                .to_owned()
                        })?
                        .map_err(|error| error.to_string())
                    })
                },
            )
            .await?;
            gate.complete_recovery(token, &uuid::Uuid::now_v7().to_string());
        }
        data.mark_terminal_committed();
    }

    if request.surface_completion && !output.result.cancelled && !data.completion_receipt_admitted {
        let source = chat_state::NotificationSource::SubagentCompleted {
            subagent_id: request.id.clone(),
            owner: subagent_notification_owner(&spawn),
        };
        let found = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            parent.received_notification_id(
                source,
                chat_state::NotificationSourceVersion::Ordinal { value: 1 },
            ),
        )
        .await
        .map_err(|_| "receipt observation timed out")?
        .ok_or("receipt observation unavailable")?;
        if found.is_some() {
            data.completion_receipt_admitted = true;
            data.receipt_in_flight = false;
        } else if data.receipt_in_flight {
            return Err(
                "original receipt admission remains in flight; no second mutation is dispatched"
                    .into(),
            );
        } else {
            admit_completion_receipt_before_result(request, &output.result, data).await;
            if !data.completion_receipt_admitted {
                return Err("completion receipt admission unconfirmed".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct ReleaseWorker(Option<std::sync::mpsc::Sender<()>>);

    impl ReleaseWorker {
        fn release(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    impl Drop for ReleaseWorker {
        fn drop(&mut self) {
            self.release();
        }
    }

    struct ControlledFsLease(Arc<AtomicBool>);

    impl Drop for ControlledFsLease {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn caller_deadline_keeps_blocked_recovery_owner_and_lease_alive() {
        let in_flight = Arc::new(AtomicBool::new(false));
        let lease = Arc::new(AtomicBool::new(false));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (worker_exit_tx, worker_exit_rx) = tokio::sync::oneshot::channel();
        let mut release_worker = ReleaseWorker(Some(release_tx));
        let caller_in_flight = in_flight.clone();
        let worker_lease = lease.clone();
        let caller = tokio::spawn(async move {
            run_owned_blocking_inner(
                caller_in_flight,
                deadline,
                move || {
                    worker_lease.store(true, Ordering::Release);
                    let _lease = ControlledFsLease(worker_lease);
                    let _ = started_tx.send(());
                    release_rx
                        .recv()
                        .map_err(|error| format!("controlled filesystem wait failed: {error}"))?;
                    Ok(())
                },
                Some(worker_exit_tx),
            )
            .await
        });
        started_rx.await.expect("blocking recovery worker started");

        tokio::time::advance(std::time::Duration::from_secs(30)).await;
        assert!(matches!(
            caller.await.expect("caller task"),
            Err(error) if error.contains("deadline elapsed")
        ));
        assert!(in_flight.load(Ordering::Acquire));
        assert!(lease.load(Ordering::Acquire));
        assert!(
            run_owned_blocking(
                in_flight.clone(),
                tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                || Ok(()),
            )
            .await
            .is_err(),
            "a retry cannot replace the owner while blocked filesystem work remains"
        );

        release_worker.release();
        worker_exit_rx
            .await
            .expect("blocking recovery worker exited after release");
        assert!(!in_flight.load(Ordering::Acquire));
        assert!(!lease.load(Ordering::Acquire));
    }
}
