//! Notification bridge: translates `tools` `ToolNotification` events
//! into `shell`'s native systems (ACP gateway, hunk tracker, file state tracker).
use crate::session::commands::SessionCommand;
use crate::session::persistence::{DurableAppendError, PersistenceHandle, PersistenceMsg};
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use agent_client_protocol::{self as acp, Client as _};
use hunk_tracker::HunkTrackerHandle;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tools::notification::types::{ToolNotification, ToolNotificationHandle};
use tools::types::output::{BashOutput, ToolOutput};
use workspace::session::file_state::FileStateTracker;
/// Configuration for the notification bridge.
pub struct NotificationBridgeConfig {
    /// ACP gateway for sending streaming updates to TUI
    pub gateway: GatewaySender,
    /// ACP session ID
    pub session_id: acp::SessionId,
    /// Hunk tracker for recording agent writes
    pub hunk_tracker_handle: HunkTrackerHandle,
    /// File state tracker for rewind functionality
    pub file_state_tracker: Arc<FileStateTracker>,
    /// Current prompt index (shared with session state)
    pub prompt_index: Arc<TokioMutex<usize>>,
    /// Working directory for path relativization
    pub cwd: PathBuf,
    /// Shared gate: when false, suppress gateway forwarding.
    /// Events are still processed for hunk tracking and file state.
    pub gateway_enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Persistence handle for FIFO ordinary writes and durable tombstone barriers.
    pub persistence: PersistenceHandle,
    /// When true, send incremental `output_delta` instead of full `output`
    /// in bash streaming updates. The client must opt in via the
    /// `grow/incrementalBashOutput` capability.
    pub incremental_bash_output: bool,
    /// Read-only Behavior state used to annotate tool notifications.
    pub behavior: Arc<parking_lot::Mutex<crate::session::behavior::BehaviorCoordinator>>,
    /// Session command channel for monitor events and task-completed injections.
    pub session_cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Resolved name of the `BackgroundTaskAction` tool. Written exactly
    /// once after the agent's toolset is finalized; read many times
    /// thereafter from the notification bridge and the session actor's
    /// between-turn drain. `None` means no such tool is registered in this
    /// toolset, which is a valid resolved state.
    pub task_output_tool_name: Arc<std::sync::OnceLock<Option<String>>>,
    /// Resolved name of the `Read` tool, used by `format_bash_completion`'s
    /// disk-pointer footer so the model can recover full bash output from
    /// `task.output_file` even when no polling tool is available. Same
    /// write-once-read-many lifecycle as `task_output_tool_name`.
    pub read_tool_name: Arc<std::sync::OnceLock<Option<String>>>,
}
/// Snapshot a shared `OnceLock` tool-name slot as a borrowed `&str`.
/// Returns `None` if the slot is still unset (toolset not yet finalized)
/// or if the resolved value is `None` (no such tool registered in this
/// toolset.
pub(crate) fn resolved_tool_name(slot: &std::sync::OnceLock<Option<String>>) -> Option<&str> {
    slot.get().and_then(|v| v.as_deref())
}
/// Stamp a bridge-emitted notification's meta before it forks into
/// persistence + broadcast — see `util::event_id::ensure_event_id_meta`.
fn stamp_event_id(config: &NotificationBridgeConfig, meta: &mut Option<acp::Meta>) {
    crate::util::event_id::ensure_event_id_meta(&config.session_id.0, meta);
}
fn stamp_scheduler_meta(
    config: &NotificationBridgeConfig,
    meta: &mut Option<acp::Meta>,
    generation: &str,
    revision: u64,
) {
    stamp_event_id(config, meta);
    let meta = meta.get_or_insert_with(acp::Meta::new);
    meta.insert("grow/schedulerGeneration".to_owned(), generation.into());
    meta.insert("grow/schedulerRevision".to_owned(), revision.into());
}
fn durable_append_landed(result: Result<(), DurableAppendError>, fact: &str) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(DurableAppendError::Committed(error)) => {
            tracing::warn!(%error, %fact, "durable projection committed with bookkeeping failure");
            Ok(())
        }
        Err(DurableAppendError::NotCommitted(error)) => {
            Err(format!("{fact} was not committed: {error}"))
        }
        Err(DurableAppendError::AcknowledgementLost(error)) => {
            Err(format!("{fact} commit status is unknown: {error}"))
        }
    }
}
async fn handle_scheduled_task_removed(
    config: &NotificationBridgeConfig,
    removed: tools::notification::ScheduledTaskRemoved,
    acknowledgement: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) -> Result<(), String> {
    tracing::info!(task_id = %removed.task_id, "Scheduled task removed");
    let result: Result<Box<serde_json::value::RawValue>, String> = async {
        let mut meta = None;
        stamp_scheduler_meta(config, &mut meta, &removed.generation, removed.revision);
        let notification = crate::extensions::notification::SessionNotification {
            session_id: config.session_id.clone(),
            update: crate::extensions::notification::SessionUpdate::ScheduledTaskDeleted {
                task_id: removed.task_id,
            },
            meta: meta.map(serde_json::Value::Object),
        };
        let params = serde_json::to_value(&notification)
            .and_then(|value| serde_json::value::to_raw_value(&value))
            .map_err(|error| format!("failed to serialize scheduled task deletion: {error}"))?;
        let update = crate::session::storage::SessionUpdate::Grow(Box::new(notification));
        if acknowledgement.is_some() {
            durable_append_landed(
                config.persistence.append_update_durably(update).await,
                "scheduler tombstone",
            )?;
        } else {
            config
                .persistence
                .tx
                .send(PersistenceMsg::Update(update))
                .map_err(|_| "session persistence stopped".to_owned())?;
        }
        Ok(params)
    }
    .await;
    match result {
        Ok(params) => {
            if let Some(acknowledgement) = acknowledgement {
                let _ = acknowledgement.send(Ok(()));
            }
            config
                .gateway
                .forward_fire_and_forget(acp::ExtNotification::new(
                    "grow/scheduled_task_deleted",
                    params.into(),
                ));
            Ok(())
        }
        Err(error) => {
            if let Some(acknowledgement) = acknowledgement {
                let _ = acknowledgement.send(Err(error.clone()));
            }
            Err(error)
        }
    }
}
/// Create a `ToolNotificationHandle` and spawn a bridge task that
/// translates notifications into shell-native systems.
pub fn spawn_notification_bridge(config: NotificationBridgeConfig) -> ToolNotificationHandle {
    let (handle, mut rx) = ToolNotificationHandle::acknowledged_channel();
    tokio::task::spawn_local(async move {
        let mut offsets: HashMap<String, usize> = HashMap::new();
        while let Some(delivery) = rx.recv().await {
            let acknowledgement = delivery.acknowledgement;
            match delivery.notification {
                ToolNotification::ScheduledTaskRemoved(removed) => {
                    if let Err(error) =
                        handle_scheduled_task_removed(&config, removed, acknowledgement).await
                    {
                        tracing::warn!(%error, "Failed to handle scheduled task removal");
                    }
                }
                notification => {
                    let result = handle_notification_with_ack(
                        &config,
                        notification,
                        &mut offsets,
                        acknowledgement.is_some(),
                    )
                    .await;
                    if let Some(acknowledgement) = acknowledgement {
                        let _ = acknowledgement.send(result);
                    } else if let Err(error) = result {
                        tracing::warn!(%error, "Failed to handle tool notification");
                    }
                }
            }
        }
        tracing::debug!("Notification bridge task exiting (sender dropped)");
    });
    handle
}
/// Emit a `CurrentModeUpdate` for the given [`BehaviorId`] — persisted to
/// `updates.jsonl` so session replay re-applies the mode, and forwarded to
/// the gateway so the pager updates live.
async fn emit_current_mode_update(
    config: &NotificationBridgeConfig,
    mode: tools::types::BehaviorId,
) {
    let mut notification = acp::SessionNotification::new(
        config.session_id.clone(),
        acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new(mode.as_id())).meta(
                serde_json::json!({
                    "grow/behavior": match config.behavior.lock().behavior() {
                        tool_types::BehaviorId::Normal => "normal",
                        tool_types::BehaviorId::Clarify => "clarify",
                        tool_types::BehaviorId::Plan => "plan",
                        tool_types::BehaviorId::Workflow => "workflow",
                        tool_types::BehaviorId::Goal => "goal",
                    },
                    "grow/planPhase": config.behavior.lock().plan_phase_label(),
                })
                .as_object()
                .cloned(),
            ),
        ),
    );
    stamp_event_id(config, &mut notification.meta);
    let _ = config.persistence.tx.send(PersistenceMsg::Update(
        crate::session::storage::SessionUpdate::Acp(Box::new(notification.clone())),
    ));
    config.gateway.forward_fire_and_forget(notification);
}

fn notification_owner(
    goal_id: Option<String>,
    goal_definition_revision: Option<u64>,
    source: &str,
) -> Result<chat_state::NotificationOwner, String> {
    match (goal_id, goal_definition_revision) {
        (None, None) => Ok(chat_state::NotificationOwner::Session),
        (Some(goal_id), Some(definition_revision))
            if !goal_id.trim().is_empty() && definition_revision > 0 =>
        {
            Ok(chat_state::NotificationOwner::Goal {
                goal_id,
                definition_revision,
            })
        }
        _ => Err(format!(
            "{source} carried an incomplete or invalid Goal owner"
        )),
    }
}

/// Handle a single notification by forwarding it to the appropriate shell system.
async fn handle_notification_with_ack(
    config: &NotificationBridgeConfig,
    notification: ToolNotification,
    offsets: &mut HashMap<String, usize>,
    require_durable_ack: bool,
) -> Result<(), String> {
    match notification {
        ToolNotification::BashOutputChunk(chunk) => {
            let (output, output_delta) = if config.incremental_bash_output {
                let prev_offset = offsets.get(&chunk.base.tool_call_id).copied().unwrap_or(0);
                let full = &chunk.base.output;
                let delta = if prev_offset <= full.len() {
                    full[prev_offset..].to_vec()
                } else {
                    full.clone()
                };
                offsets.insert(chunk.base.tool_call_id.clone(), full.len());
                (Vec::new(), Some(delta))
            } else {
                (chunk.base.output.clone(), None)
            };
            let bash_output = ToolOutput::Bash(BashOutput {
                output_for_prompt: BashOutput::make_output_for_prompt(&String::from_utf8_lossy(
                    &chunk.base.output,
                )),
                output,
                exit_code: 0,
                command: chunk.base.command.clone(),
                truncated: chunk.base.truncated,
                signal: None,
                timed_out: false,
                description: None,
                current_dir: chunk.base.cwd.to_string_lossy().to_string(),
                output_file: String::new(),
                total_bytes: chunk.base.total_bytes,
                output_delta,
                was_bare_echo: false,
            });
            let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                acp::ToolCallId::new(chunk.base.tool_call_id.clone()),
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::InProgress))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(
                            String::from_utf8_lossy(&chunk.base.output).into_owned(),
                        )),
                    )]))
                    .raw_output(serde_json::to_value(&bash_output).ok()),
            ));
            let mut notification = acp::SessionNotification::new(config.session_id.clone(), update);
            stamp_event_id(config, &mut notification.meta);
            let _ = config.persistence.tx.send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Acp(Box::new(notification.clone())),
            ));
            if config
                .gateway_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let _ = config.gateway.session_notification(notification).await;
            }
        }
        ToolNotification::BashExecutionComplete(complete) => {
            offsets.remove(&complete.base.tool_call_id);
            tracing::debug!(
                tool_call_id = %complete.base.tool_call_id,
                exit_code = ?complete.exit_code,
                "Bash execution complete notification received"
            );
        }
        ToolNotification::BashExecutionTimeout(timeout) => {
            tracing::debug!(
                tool_call_id = %timeout.base.tool_call_id,
                elapsed = ?timeout.elapsed,
                "Bash execution timeout notification received"
            );
        }
        ToolNotification::BashExecutionFailed(failed) => {
            tracing::warn!(
                tool_call_id = %failed.tool_call_id,
                error = %failed.error,
                "Bash execution failed notification received"
            );
        }
        ToolNotification::BashExecutionBackgrounded(bg) => {
            match (bg.goal_id.as_deref(), bg.goal_definition_revision) {
                (Some(goal_id), Some(definition_revision)) if definition_revision > 0 => {
                    // The backgrounded event is ordered before terminal receipts
                    // on this bridge channel. Register admission ownership now,
                    // rather than waiting for the tool result path.
                    config
                        .session_cmd_tx
                        .send(SessionCommand::RecordGoalOwnedTaskIds {
                            goal_id: goal_id.to_owned(),
                            definition_revision,
                            task_ids: vec![bg.task_id.clone()],
                        })
                        .map_err(|_| {
                            "session stopped before Goal task ownership admission".to_owned()
                        })?;
                }
                (None, None) => {}
                _ => {
                    return Err(
                        "background task admission carried an incomplete Goal owner".to_owned()
                    );
                }
            }
            tracing::debug!(
                tool_call_id = %bg.base.tool_call_id,
                task_id = %bg.task_id,
                command = %bg.base.command,
                output_file = %bg.output_file.display(),
                "Bash execution backgrounded notification received — forwarding to TUI"
            );
            let mut notification = crate::extensions::notification::SessionNotification {
                session_id: config.session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::TaskBackgrounded {
                    tool_call_id: bg.base.tool_call_id.clone(),
                    task_id: bg.task_id.clone(),
                    command: bg.base.command.clone(),
                    cwd: bg.base.cwd.to_string_lossy().to_string(),
                    output_file: bg.output_file.to_string_lossy().to_string(),
                    monitor_description: bg.monitor_description.clone(),
                    description: bg.description.clone(),
                },
                meta: None,
            };
            {
                let mut meta_map = None;
                stamp_event_id(config, &mut meta_map);
                notification.meta = meta_map.map(serde_json::Value::Object);
            }
            let _ = config.persistence.tx.send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone())),
            ));
            let params = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
                .ok();
            if let Some(params) = params {
                let ext_notification =
                    acp::ExtNotification::new("grow/task_backgrounded", params.into());
                config.gateway.forward_fire_and_forget(ext_notification);
            }
        }
        ToolNotification::FileWritten(written) => {
            let prompt_index = *config.prompt_index.lock().await;
            config.hunk_tracker_handle.record_agent_write(
                written.absolute_path.clone(),
                written.content.clone(),
                prompt_index,
                written.previous_content.clone(),
            );
            if written.previous_content.is_some() || written.is_new_file {
                config
                    .file_state_tracker
                    .add_before_snapshot_for_prompt(
                        prompt_index,
                        &written.absolute_path,
                        &config.cwd,
                        written.previous_content,
                    )
                    .await;
            }
            tracing::debug!(
                path = %written.absolute_path.display(),
                is_new_file = written.is_new_file,
                "FileWritten notification forwarded to hunk tracker"
            );
        }
        ToolNotification::TaskCompleted(task_snapshot) => {
            let is_monitor = task_snapshot.kind == tools::computer::types::TaskKind::Monitor;
            let task_id = task_snapshot.task_id.clone();
            let owner = notification_owner(
                task_snapshot.goal_id.clone(),
                task_snapshot.goal_definition_revision,
                "completed task",
            )?;
            if !task_snapshot.block_waited && !task_snapshot.explicitly_killed {
                let tool_name = resolved_tool_name(&config.task_output_tool_name);
                let read_name = resolved_tool_name(&config.read_tool_name);
                let body = if is_monitor {
                    tools::reminders::task_completion::format_monitor_completion(
                        &task_snapshot,
                        tool_name,
                    )
                } else {
                    tools::reminders::task_completion::format_bash_completion(
                        &task_snapshot,
                        tool_name,
                        read_name,
                    )
                };
                let (respond_to, receipt) = if require_durable_ack {
                    let (respond_to, receipt) = tokio::sync::oneshot::channel();
                    (Some(respond_to), Some(receipt))
                } else {
                    (None, None)
                };
                config
                    .session_cmd_tx
                    .send(SessionCommand::ReceiveNotification {
                        source: chat_state::NotificationSource::TaskCompleted {
                            task_id: task_id.clone(),
                            task_kind: if is_monitor {
                                chat_state::NotificationTaskKind::Monitor
                            } else {
                                chat_state::NotificationTaskKind::Task
                            },
                            owner,
                        },
                        source_version: chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        body,
                        respond_to,
                    })
                    .map_err(|_| "session stopped before task completion admission".to_owned())?;
                if let Some(receipt) = receipt {
                    receipt
                        .await
                        .map_err(|_| {
                            "task completion admission acknowledgement was dropped".to_owned()
                        })?
                        .map(|_| ())?;
                }
            }
            let mut notification = crate::extensions::notification::SessionNotification {
                session_id: config.session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::TaskCompleted {
                    task_snapshot,
                },
                meta: None,
            };
            {
                let mut meta_map = None;
                stamp_event_id(config, &mut meta_map);
                notification.meta = meta_map.map(serde_json::Value::Object);
            }
            let update =
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone()));
            if require_durable_ack {
                durable_append_landed(
                    config.persistence.append_update_durably(update).await,
                    "task completion UI projection",
                )?;
            } else {
                config
                    .persistence
                    .tx
                    .send(PersistenceMsg::Update(update))
                    .map_err(|_| "session persistence stopped".to_owned())?;
            }
            let params = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
                .ok();
            if let Some(params) = params {
                let notification: acp::ExtNotification =
                    acp::ExtNotification::new("grow/task_completed", params.into());
                config.gateway.forward_fire_and_forget(notification);
            }
            let _ = config
                .session_cmd_tx
                .send(SessionCommand::DispatchNotificationHook {
                    notification_type: "task_complete".into(),
                    message: Some(format!("Background task completed: {task_id}")),
                    title: None,
                    level: Some("info".into()),
                });
        }
        ToolNotification::UserQuestionAsked(asked) => {
            tracing::info!(
                tool_call_id = %asked.tool_call_id,
                "User question asked"
            );
        }
        ToolNotification::LspServerStarting(s) => {
            tracing::debug!(server = %s.server_name, command = %s.command, "LSP server starting");
        }
        ToolNotification::LspServerReady(s) => {
            tracing::info!(server = %s.server_name, "LSP server ready");
        }
        ToolNotification::LspServerCrashed(s) => {
            tracing::warn!(server = %s.server_name, "LSP server crashed");
        }
        ToolNotification::LspServerRetrying(s) => {
            tracing::warn!(
                server = %s.server_name,
                attempt = s.attempt,
                max_restarts = s.max_restarts,
                backoff_ms = s.backoff_ms,
                "LSP server retrying"
            );
        }
        ToolNotification::LspServerFailed(s) => {
            tracing::error!(server = %s.server_name, error = %s.error, "LSP server failed");
        }
        ToolNotification::ScheduledTaskFired(fired) => {
            tracing::info!(
                task_id = %fired.task_id,
                schedule = %fired.human_schedule,
                subagent_id = %fired.subagent_id,
                "Scheduled task fired"
            );
            let mut meta = None;
            stamp_scheduler_meta(config, &mut meta, &fired.generation, fired.revision);
            let fired_notif = crate::extensions::notification::SessionNotification {
                session_id: config.session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::ScheduledTaskFired {
                    task_id: fired.task_id,
                    prompt: fired.prompt,
                    human_schedule: fired.human_schedule,
                    next_fire_at: fired.next_fire_at,
                    subagent_id: fired.subagent_id,
                },
                meta: meta.map(serde_json::Value::Object),
            };
            if let Ok(params) =
                serde_json::to_value(&fired_notif).and_then(|v| serde_json::value::to_raw_value(&v))
            {
                config
                    .gateway
                    .forward_fire_and_forget(acp::ExtNotification::new(
                        "grow/scheduled_task_fired",
                        params.into(),
                    ));
            }
        }
        ToolNotification::MonitorEvent(event) => {
            let my_session = config.session_id.0.as_ref();
            if event.owner_session_id.as_deref() != Some(my_session) {
                tracing::warn!(
                    task_id = %event.task_id,
                    description = %event.description,
                    monitor_owner = ?event.owner_session_id,
                    bridge_session = %my_session,
                    "Dropped monitor event without this bridge's exact session owner"
                );
                return Ok(());
            }
            let owner = notification_owner(
                event.goal_id.clone(),
                event.goal_definition_revision,
                "monitor event",
            )?;
            tracing::debug!(
                task_id = %event.task_id,
                description = %event.description,
                "Monitor event received, injecting into session"
            );
            let notification = crate::extensions::notification::SessionNotification {
                session_id: config.session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::MonitorEvent {
                    task_id: event.task_id.clone(),
                    description: event.description.clone(),
                    event_text: event.raw_text.clone(),
                },
                meta: None,
            };
            let params = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
                .ok();
            if let Some(params) = params {
                config
                    .gateway
                    .forward_fire_and_forget(acp::ExtNotification::new(
                        "grow/monitor_event",
                        params.into(),
                    ));
            }
            let _ = config
                .session_cmd_tx
                .send(SessionCommand::ReceiveNotification {
                    source: chat_state::NotificationSource::MonitorProgress {
                        task_id: event.task_id.clone(),
                        owner,
                    },
                    source_version: chat_state::NotificationSourceVersion::Opaque {
                        value: uuid::Uuid::now_v7().to_string(),
                    },
                    body: event.event_text,
                    respond_to: None,
                });
        }
        ToolNotification::ScheduledTaskRemoved(removed) => {
            if let Err(error) = handle_scheduled_task_removed(config, removed, None).await {
                tracing::warn!(%error, "Failed to handle scheduled task removal");
            }
        }
        ToolNotification::ScheduledTaskCreated(created) => {
            tracing::info!(task_id = %created.task_id, "Scheduled task created");
            let mut meta = None;
            stamp_scheduler_meta(config, &mut meta, &created.generation, created.revision);
            let notification = crate::extensions::notification::SessionNotification {
                session_id: config.session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::ScheduledTaskCreated {
                    task_id: created.task_id,
                    prompt: created.prompt,
                    human_schedule: created.human_schedule,
                    next_fire_at: created.next_fire_at,
                },
                meta: meta.map(serde_json::Value::Object),
            };
            let _ = config.persistence.tx.send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone())),
            ));
            if let Ok(params) = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
            {
                config
                    .gateway
                    .forward_fire_and_forget(acp::ExtNotification::new(
                        "grow/scheduled_task_created",
                        params.into(),
                    ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
async fn handle_notification(
    config: &NotificationBridgeConfig,
    notification: ToolNotification,
    offsets: &mut HashMap<String, usize>,
) {
    handle_notification_with_ack(config, notification, offsets, false)
        .await
        .expect("unacknowledged test notification should be handled");
}
#[cfg(test)]
mod tests {
    use super::*;
    use tools::computer::types::TaskKind;
    use tools::types::TaskSnapshot;
    fn make_test_config() -> (
        NotificationBridgeConfig,
        mpsc::UnboundedReceiver<SessionCommand>,
    ) {
        let (config, _gateway_rx, mut persistence_rx, session_cmd_rx) = make_test_config_full();
        tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
        (config, session_cmd_rx)
    }
    #[allow(clippy::type_complexity)]
    fn make_test_config_full() -> (
        NotificationBridgeConfig,
        mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
        mpsc::UnboundedReceiver<PersistenceMsg>,
        mpsc::UnboundedReceiver<SessionCommand>,
    ) {
        make_test_config_full_raw()
    }
    #[allow(clippy::type_complexity)]
    fn make_test_config_full_raw() -> (
        NotificationBridgeConfig,
        mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
        mpsc::UnboundedReceiver<PersistenceMsg>,
        mpsc::UnboundedReceiver<SessionCommand>,
    ) {
        let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
        let gateway = acp_transport::AcpAgentGatewaySender::new(gateway_tx);
        let (session_cmd_tx, session_cmd_rx) = mpsc::unbounded_channel();
        let (persistence_tx, persistence_rx) = mpsc::unbounded_channel();
        let config = NotificationBridgeConfig {
            gateway,
            session_id: acp::SessionId::new("test-session"),
            hunk_tracker_handle: HunkTrackerHandle::noop(),
            file_state_tracker: Arc::new(FileStateTracker::new()),
            prompt_index: Arc::new(TokioMutex::new(0)),
            cwd: PathBuf::from("/tmp"),
            gateway_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            persistence: PersistenceHandle::from_sender_for_test(persistence_tx),
            incremental_bash_output: false,
            behavior: Arc::new(parking_lot::Mutex::new(
                crate::session::behavior::BehaviorCoordinator::new(),
            )),
            session_cmd_tx,
            task_output_tool_name: Arc::new(std::sync::OnceLock::new()),
            read_tool_name: Arc::new(std::sync::OnceLock::new()),
        };
        (config, gateway_rx, persistence_rx, session_cmd_rx)
    }
    fn make_task_snapshot(task_id: &str, kind: TaskKind) -> TaskSnapshot {
        TaskSnapshot {
            task_id: task_id.into(),
            command: "echo test".into(),
            display_command: None,
            cwd: String::new(),
            start_time: std::time::SystemTime::now(),
            end_time: Some(std::time::SystemTime::now()),
            output: String::new(),
            output_file: PathBuf::new(),
            truncated: false,
            exit_code: Some(0),
            signal: None,
            completed: true,
            kind,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            goal_definition_revision: None,
            description: None,
            is_backgrounded: false,
        }
    }
    #[tokio::test]
    async fn bash_task_completed_injects_bash_task_completed_source() {
        let (config, mut cmd_rx) = make_test_config();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let snapshot = make_task_snapshot("bg-123", TaskKind::Bash);
        let notification = ToolNotification::TaskCompleted(snapshot);
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        let command = cmd_rx.try_recv().expect("expected Prompt");
        match command {
            SessionCommand::ReceiveNotification { source, body, .. } => {
                assert!(matches!(
                    source,
                    chat_state::NotificationSource::TaskCompleted { task_id, .. }
                        if task_id == "bg-123"
                ));
                assert!(body.contains("bg-123"));
                assert!(body.contains("exit code: 0"));
                assert!(body.contains(r#"get_command_or_subagent_output("bg-123")"#));
                assert!(!body.contains(r#"get_task_output("bg-123")"#));
            }
            _ => panic!("expected ReceiveNotification"),
        }
        let cmd3 = cmd_rx
            .try_recv()
            .expect("expected DispatchNotificationHook for task_complete");
        match cmd3 {
            SessionCommand::DispatchNotificationHook {
                notification_type,
                message,
                ..
            } => {
                assert_eq!(notification_type, "task_complete");
                assert_eq!(
                    message.as_deref(),
                    Some("Background task completed: bg-123")
                );
            }
            _ => panic!("expected DispatchNotificationHook"),
        }
    }
    /// Every unsurfaced background completion enters the durable inbox.
    #[tokio::test]
    async fn bash_task_completed_emits_one_durable_receipt() {
        let (config, mut cmd_rx) = make_test_config();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let snapshot = make_task_snapshot("bg-normal", TaskKind::Bash);
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::ReceiveNotification { .. })
        ));
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::DispatchNotificationHook { .. })
        ));
    }

    /// The terminal snapshot carries Goal ownership even when the process
    /// exits before the tool can emit its Backgrounded notification.
    #[tokio::test]
    async fn instantaneous_goal_task_completion_uses_snapshot_owner() {
        let (config, mut cmd_rx) = make_test_config();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let mut snapshot = make_task_snapshot("instant-goal", TaskKind::Bash);
        snapshot.goal_id = Some("goal-fast".into());
        snapshot.goal_definition_revision = Some(1);
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        let SessionCommand::ReceiveNotification { source, .. } = cmd_rx
            .try_recv()
            .expect("instant completion must emit a receipt")
        else {
            panic!("expected ReceiveNotification");
        };
        assert_eq!(
            source.owner(),
            chat_state::NotificationOwner::Goal {
                goal_id: "goal-fast".into(),
                definition_revision: 1,
            }
        );
    }

    #[tokio::test]
    async fn incomplete_goal_task_owner_is_rejected_before_publication() {
        let (config, mut gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut snapshot = make_task_snapshot("invalid-goal-task", TaskKind::Bash);
        snapshot.goal_id = Some("goal-1".into());
        let mut offsets = HashMap::new();

        let error = handle_notification_with_ack(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
            false,
        )
        .await
        .unwrap_err();

        assert!(error.contains("incomplete or invalid Goal owner"));
        assert!(cmd_rx.try_recv().is_err());
        assert!(gateway_rx.try_recv().is_err());
        assert!(persistence_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn acknowledged_task_completion_waits_for_timeline_admission() {
        let (config, _gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut offsets = HashMap::new();
        let handle = handle_notification_with_ack(
            &config,
            ToolNotification::TaskCompleted(make_task_snapshot("bg-ack", TaskKind::Bash)),
            &mut offsets,
            true,
        );
        let acknowledge = async {
            let SessionCommand::ReceiveNotification {
                respond_to: Some(respond_to),
                ..
            } = cmd_rx.recv().await.expect("completion admission command")
            else {
                panic!("expected acknowledged completion admission");
            };
            respond_to.send(Ok("notification-1".into())).unwrap();
            let PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. } =
                persistence_rx.recv().await.expect("durable UI projection")
            else {
                panic!("expected durable task completion projection");
            };
            respond_to.send(Ok(())).unwrap();
        };
        let (handled, ()) = tokio::join!(handle, acknowledge);
        handled.expect("producer acknowledgement follows durable Timeline admission");
    }
    fn take_task_completed_notification(
        gateway_rx: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    ) -> Option<serde_json::Value> {
        while let Ok(msg) = gateway_rx.try_recv() {
            if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                && args.request.method.as_ref() == "grow/task_completed"
            {
                let v: serde_json::Value = serde_json::from_str(args.request.params.get()).ok()?;
                return Some(v);
            }
        }
        None
    }
    #[tokio::test]
    async fn task_completed_receipt_preserves_ui_and_storage_projections() {
        let (config, mut gateway_rx, _persistence_rx, mut cmd_rx) = make_test_config_full();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(make_task_snapshot("bg-wake", TaskKind::Bash)),
            &mut offsets,
        )
        .await;
        assert!(matches!(
            cmd_rx.recv().await,
            Some(SessionCommand::ReceiveNotification { .. })
        ));
        let notification = take_task_completed_notification(&mut gateway_rx)
            .expect("completion notification must be emitted");
        assert!(notification["update"].get("will_wake").is_none());
        let (config, mut gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(make_task_snapshot("bg-declined", TaskKind::Bash)),
            &mut offsets,
        )
        .await;
        let notification = take_task_completed_notification(&mut gateway_rx)
            .expect("completion must still emit a UI notification");
        assert!(notification["update"].get("will_wake").is_none());
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::ReceiveNotification { .. })
        ));
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::DispatchNotificationHook { .. })
        ));
        let mut persisted = false;
        while let Ok(message) = persistence_rx.try_recv() {
            if let PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(update)) =
                message
                && matches!(
                    &update.update,
                    crate::extensions::notification::SessionUpdate::TaskCompleted { .. }
                )
            {
                persisted = true;
            }
        }
        assert!(
            persisted,
            "completion must still persist grow/task_completed"
        );
    }
    #[tokio::test]
    async fn task_completed_rejects_without_uncommitted_projection_when_session_is_closed() {
        let (config, mut gateway_rx, mut persistence_rx, cmd_rx) = make_test_config_full_raw();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        drop(cmd_rx);
        let mut offsets = HashMap::new();
        let result = handle_notification_with_ack(
            &config,
            ToolNotification::TaskCompleted(make_task_snapshot("bg-dead", TaskKind::Bash)),
            &mut offsets,
            true,
        )
        .await;
        assert!(matches!(result, Err(error) if error.contains("session stopped")));
        assert!(
            take_task_completed_notification(&mut gateway_rx).is_none(),
            "a completion rejected before Timeline admission must not be presented as delivered"
        );
        assert!(
            persistence_rx.try_recv().is_err(),
            "a completion rejected before Timeline admission must not be persisted as delivered"
        );
    }
    /// Natural monitor exit (including exit code 0) must immediate-auto-wake
    /// the same way bash does — not only via the idle-gated MonitorEvent path.
    /// Also drops queued MonitorEvents so a second NotificationDrain turn is
    /// not started for the same completion.
    #[tokio::test]
    async fn monitor_task_completed_auto_wakes_with_monitor_ended_message() {
        let (config, mut cmd_rx) = make_test_config();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let mut snapshot = make_task_snapshot("mon-456", TaskKind::Monitor);
        snapshot.display_command = Some("[monitor] watch deploy".into());
        snapshot.command = "tail -f deploy.log".into();
        snapshot.exit_code = Some(0);
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        let cmd = cmd_rx.try_recv().expect("expected ReceiveNotification");
        match cmd {
            SessionCommand::ReceiveNotification { source, body, .. } => {
                assert!(matches!(
                    source,
                    chat_state::NotificationSource::TaskCompleted { task_id, .. }
                        if task_id == "mon-456"
                ));
                let text = body.as_str();
                assert!(
                    text.contains("[monitor ended: exited (code 0)]"),
                    "auto-wake must carry the terminal ended wording: {text}"
                );
                assert!(
                    text.contains("watch deploy"),
                    "auto-wake should include the monitor description: {text}"
                );
                assert!(
                    text.contains("get_command_or_subagent_output(\"mon-456\")"),
                    "auto-wake should point at the poll tool: {text}"
                );
            }
            _ => panic!("expected ReceiveNotification for natural monitor exit"),
        }
        // Durable receipt admission replaces the old queue-drop side channel;
        // the actor's inbox projection suppresses duplicate monitor progress.
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::DispatchNotificationHook { .. })
        ));
    }
    #[tokio::test]
    async fn quiet_monitor_completion_emits_receipt_and_persists_projection() {
        let (config, _gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        config
            .task_output_tool_name
            .set(Some("get_command_or_subagent_output".to_string()))
            .expect("slot is fresh in this test fixture");
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(make_task_snapshot("mon-declined", TaskKind::Monitor)),
            &mut offsets,
        )
        .await;
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::ReceiveNotification { .. })
        ));
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::DispatchNotificationHook { .. })
        ));
        assert!(cmd_rx.try_recv().is_err());
        let mut persisted_completion = false;
        while let Ok(message) = persistence_rx.try_recv() {
            if let PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(update)) =
                message
                && matches!(
                    &update.update,
                    crate::extensions::notification::SessionUpdate::TaskCompleted { .. }
                )
            {
                persisted_completion = true;
            }
        }
        assert!(persisted_completion);
    }
    /// Late progress still enters the durable inbox. Timeline folding owns
    /// suppression after a terminal monitor receipt, including after reload.
    #[tokio::test]
    async fn monitor_event_emits_progress_receipt_for_timeline_folding() {
        let (config, mut cmd_rx) = make_test_config();
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::MonitorEvent(tools::notification::types::MonitorEvent {
                task_id: "mon-done".into(),
                description: "short exit".into(),
                event_text: "<monitor-event>done</monitor-event>".into(),
                raw_text: "done".into(),
                owner_session_id: Some("test-session".into()),
                goal_id: None,
                goal_definition_revision: None,
            }),
            &mut offsets,
        )
        .await;
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::ReceiveNotification {
                source: chat_state::NotificationSource::MonitorProgress { task_id, .. },
                ..
            }) if task_id == "mon-done"
        ));
    }
    /// Explicit kill of a monitor still skips auto-wake — the model already
    /// got the kill_task tool result.
    #[tokio::test]
    async fn monitor_explicitly_killed_skips_auto_wake() {
        let (config, mut cmd_rx) = make_test_config();
        let mut snapshot = make_task_snapshot("mon-killed", TaskKind::Monitor);
        snapshot.explicitly_killed = true;
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        match cmd_rx
            .try_recv()
            .expect("expected DispatchNotificationHook for task_complete")
        {
            SessionCommand::DispatchNotificationHook {
                notification_type, ..
            } => {
                assert_eq!(notification_type, "task_complete")
            }
            _ => panic!("unexpected session command"),
        }
        assert!(
            cmd_rx.try_recv().is_err(),
            "explicitly-killed monitor must not auto-wake"
        );
    }
    #[tokio::test]
    async fn scheduled_task_created_is_persisted() {
        let (config, _gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        let notification = ToolNotification::ScheduledTaskCreated(
            tools::notification::types::ScheduledTaskCreated {
                task_id: "loop-1".into(),
                prompt: "check deploy".into(),
                human_schedule: "every 5 minutes".into(),
                next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                generation: "generation-a".into(),
                revision: 1,
            },
        );
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        let msg = persistence_rx
            .try_recv()
            .expect("scheduled_task_created must be persisted");
        match msg {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(notif)) => {
                assert!(matches!(
                    &notif.update,
                    crate::extensions::notification::SessionUpdate::ScheduledTaskCreated { .. }
                ));
                let meta = notif.meta.as_ref().expect("scheduler metadata");
                assert_eq!(meta["grow/schedulerGeneration"], "generation-a");
                assert_eq!(meta["grow/schedulerRevision"], 1);
                assert!(
                    notif
                        .meta
                        .as_ref()
                        .and_then(|m| m.get("eventId"))
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| id.starts_with("test-session-")),
                    "persisted Grow bridge lines must carry an eventId"
                );
            }
            _ => panic!("expected PersistenceMsg::Update(Grow(ScheduledTaskCreated))"),
        }
    }
    /// Persisted⇒stamped contract at the bridge's highest-frequency emitter:
    /// the persisted bash-output line carries an `eventId`, and the live
    /// broadcast carries the SAME id (the meta is minted before the
    /// persist/broadcast fork — divergent ids would re-deliver the line on a
    /// cursor reconnect).
    #[tokio::test]
    async fn bash_output_chunk_persists_and_broadcasts_one_event_id() {
        let (config, mut gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        let notification =
            ToolNotification::BashOutputChunk(tools::notification::types::BashOutputChunk {
                base: tools::notification::types::BashNotificationBase {
                    tool_call_id: "call-1".into(),
                    command: "echo hi".into(),
                    output: b"hi\n".to_vec(),
                    total_bytes: 3,
                    truncated: false,
                    cwd: PathBuf::from("/tmp"),
                },
            });
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        let persisted_id = match persistence_rx.try_recv().expect("chunk must be persisted") {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(notif)) => notif
                .meta
                .as_ref()
                .and_then(|m| m.get("eventId"))
                .and_then(|v| v.as_str())
                .expect("persisted ACP bridge lines must carry an eventId")
                .to_string(),
            other => panic!("expected PersistenceMsg::Update(Acp(..)), got {other:?}"),
        };
        let broadcast_id = match gateway_rx.try_recv().expect("chunk must be broadcast") {
            acp_transport::AcpClientMessage::SessionNotification(args) => args
                .request
                .meta
                .as_ref()
                .and_then(|m| m.get("eventId"))
                .and_then(|v| v.as_str())
                .expect("broadcast must carry the eventId")
                .to_string(),
            other => panic!("expected SessionNotification, got {other:?}"),
        };
        assert_eq!(persisted_id, broadcast_id);
    }
    #[tokio::test]
    async fn scheduled_task_removed_is_persisted() {
        let (config, _gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        let removed = tools::notification::ScheduledTaskRemoved {
            task_id: "loop-1".into(),
            generation: "generation-a".into(),
            revision: 2,
        };
        handle_scheduled_task_removed(&config, removed, None)
            .await
            .unwrap();
        let msg = persistence_rx
            .try_recv()
            .expect("scheduled_task_removed must be persisted");
        match msg {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(notif)) => {
                assert!(matches!(
                    &notif.update,
                    crate::extensions::notification::SessionUpdate::ScheduledTaskDeleted { .. }
                ));
                assert!(
                    grow_persisted_event_id(&notif).is_some(),
                    "the persisted deletion line must be stamped"
                );
                let meta = notif.meta.as_ref().expect("scheduler metadata");
                assert_eq!(meta["grow/schedulerGeneration"], "generation-a");
                assert_eq!(meta["grow/schedulerRevision"], 2);
            }
            _ => panic!("expected PersistenceMsg::Update(Grow(ScheduledTaskDeleted))"),
        }
    }
    #[tokio::test]
    async fn acknowledged_scheduler_removal_appends_before_ack_and_broadcast() {
        let (config, mut gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        let removed = tools::notification::ScheduledTaskRemoved {
            task_id: "loop-ack".into(),
            generation: "generation-a".into(),
            revision: 17,
        };
        let (acknowledgement, mut receipt) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let persistence = async {
            let PersistenceMsg::AppendUpdateDurablyAndAck {
                update: crate::session::storage::SessionUpdate::Grow(notification),
                respond_to,
            } = persistence_rx.recv().await.expect("durable append")
            else {
                panic!("expected durable scheduler tombstone");
            };
            assert_eq!(notification.meta.unwrap()["grow/schedulerRevision"], 17);
            assert!(gateway_rx.try_recv().is_err());
            assert!(matches!(
                receipt.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            respond_to.send(Ok(())).unwrap();
            receipt.await.unwrap().unwrap();
        };
        let (result, ()) = tokio::join!(
            handle_scheduled_task_removed(&config, removed, Some(acknowledgement)),
            persistence,
        );
        result.unwrap();
        assert!(matches!(
            gateway_rx.try_recv(),
            Ok(acp_transport::AcpClientMessage::ExtNotification(_))
        ));
    }
    fn grow_persisted_event_id(
        notif: &crate::extensions::notification::SessionNotification,
    ) -> Option<String> {
        notif
            .meta
            .as_ref()
            .and_then(|m| m.get("eventId"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
    /// Per-site stamp pins for the bridge emitters not covered by the
    /// representative chokepoint tests: deleting any one `stamp_event_id`
    /// call must fail a test (an id-less persisted line silently disables
    /// incremental reconnect for the session).
    #[tokio::test]
    async fn task_backgrounded_persisted_line_is_stamped() {
        let (config, _gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        let notification = ToolNotification::BashExecutionBackgrounded(
            tools::notification::types::BashExecutionBackgrounded {
                base: tools::notification::types::BashNotificationBase {
                    tool_call_id: "call-bg".into(),
                    command: "sleep 100".into(),
                    output: Vec::new(),
                    total_bytes: 0,
                    truncated: false,
                    cwd: PathBuf::from("/tmp"),
                },
                output_file: PathBuf::from("/tmp/out.log"),
                task_id: "task-bg".into(),
                goal_id: Some("goal-1".into()),
                goal_definition_revision: Some(1),
                monitor_description: None,
                description: None,
            },
        );
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        assert!(matches!(
            cmd_rx.try_recv().expect("Goal ownership must be admitted first"),
            SessionCommand::RecordGoalOwnedTaskIds { goal_id, definition_revision, task_ids }
                if goal_id == "goal-1" && definition_revision == 1 && task_ids == vec!["task-bg"]
        ));
        match persistence_rx.try_recv().expect("must persist") {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(notif)) => {
                assert!(grow_persisted_event_id(&notif).is_some());
            }
            _ => panic!("expected Grow update"),
        }
    }
    #[tokio::test]
    async fn task_completed_persisted_line_is_stamped() {
        let (config, _gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        let snapshot = make_task_snapshot("mon-1", TaskKind::Monitor);
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        match persistence_rx.try_recv().expect("must persist") {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(notif)) => {
                assert!(grow_persisted_event_id(&notif).is_some());
            }
            _ => panic!("expected Grow update"),
        }
    }
    #[tokio::test]
    async fn current_mode_update_persisted_line_is_stamped() {
        let (config, _gateway_rx, mut persistence_rx, _cmd_rx) = make_test_config_full();
        emit_current_mode_update(&config, tools::types::BehaviorId::Plan).await;
        match persistence_rx.try_recv().expect("must persist") {
            PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(notif)) => {
                assert!(matches!(
                    notif.update,
                    acp::SessionUpdate::CurrentModeUpdate(_)
                ));
                assert!(
                    notif
                        .meta
                        .as_ref()
                        .and_then(|m| m.get("eventId"))
                        .and_then(|v| v.as_str())
                        .is_some(),
                    "the persisted mode line must be stamped"
                );
            }
            _ => panic!("expected Acp update"),
        }
    }
    #[test]
    fn durable_append_mapping_respects_commit_disposition() {
        assert!(
            durable_append_landed(
                Err(DurableAppendError::Committed(std::io::Error::other(
                    "summary failed"
                ),)),
                "test fact"
            )
            .is_ok()
        );
        for failure in [
            DurableAppendError::NotCommitted(std::io::Error::other("append failed")),
            DurableAppendError::AcknowledgementLost(std::io::Error::other("lost")),
        ] {
            assert!(durable_append_landed(Err(failure), "test fact").is_err());
        }
    }
    #[tokio::test]
    async fn scheduled_task_fired_updates_ui_without_waking_main_agent() {
        let (config, mut gateway_rx, mut persistence_rx, mut cmd_rx) = make_test_config_full();
        let notification =
            ToolNotification::ScheduledTaskFired(tools::notification::types::ScheduledTaskFired {
                task_id: "loop-1".into(),
                prompt: "check deploy".into(),
                human_schedule: "every 5 minutes".into(),
                next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                subagent_id: "subagent-1".into(),
                generation: "generation-a".into(),
                revision: 3,
            });
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        assert!(
            persistence_rx.try_recv().is_err(),
            "scheduled_task_fired must NOT be persisted (recurring \u{2192} unbounded log growth)"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "loop fire is only lifecycle/UI state; the loop subagent completion owns model wake"
        );
        let fired = gateway_rx
            .try_recv()
            .expect("scheduled fire must be broadcast");
        let acp_transport::AcpClientMessage::ExtNotification(fired) = fired else {
            panic!("expected scheduler fire notification");
        };
        let value: serde_json::Value = serde_json::from_str(fired.request.params.get()).unwrap();
        assert_eq!(value["_meta"]["grow/schedulerGeneration"], "generation-a");
        assert_eq!(value["_meta"]["grow/schedulerRevision"], 3);
    }
    fn make_monitor_event_notification(task_id: &str, owner: Option<&str>) -> ToolNotification {
        ToolNotification::MonitorEvent(tools::notification::types::MonitorEvent {
            task_id: task_id.into(),
            description: "errors in deploy.log".into(),
            event_text: format!("<monitor-event task_id=\"{task_id}\">boom</monitor-event>"),
            raw_text: "boom".into(),
            owner_session_id: owner.map(str::to_string),
            goal_id: None,
            goal_definition_revision: None,
        })
    }

    #[tokio::test]
    async fn monitor_progress_keeps_the_task_goal_owner() {
        let (config, mut cmd_rx) = make_test_config();
        let mut offsets = HashMap::new();
        handle_notification(
            &config,
            ToolNotification::MonitorEvent(tools::notification::types::MonitorEvent {
                task_id: "goal-monitor".into(),
                description: "watch release".into(),
                event_text: "<monitor-event>ready</monitor-event>".into(),
                raw_text: "ready".into(),
                owner_session_id: Some("test-session".into()),
                goal_id: Some("goal-1".into()),
                goal_definition_revision: Some(1),
            }),
            &mut offsets,
        )
        .await;
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(SessionCommand::ReceiveNotification {
                source: chat_state::NotificationSource::MonitorProgress {
                    task_id,
                    owner: chat_state::NotificationOwner::Goal {
                        goal_id,
                        definition_revision,
                    },
                },
                ..
            }) if task_id == "goal-monitor"
                && goal_id == "goal-1"
                && definition_revision == 1
        ));
    }

    #[tokio::test]
    async fn incomplete_goal_monitor_owner_is_rejected_before_publication() {
        let (config, mut gateway_rx, _persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut offsets = HashMap::new();

        let error = handle_notification_with_ack(
            &config,
            ToolNotification::MonitorEvent(tools::notification::types::MonitorEvent {
                task_id: "invalid-goal-monitor".into(),
                description: "watch release".into(),
                event_text: "<monitor-event>ready</monitor-event>".into(),
                raw_text: "ready".into(),
                owner_session_id: Some("test-session".into()),
                goal_id: Some("goal-1".into()),
                goal_definition_revision: None,
            }),
            &mut offsets,
            false,
        )
        .await
        .unwrap_err();

        assert!(error.contains("incomplete or invalid Goal owner"));
        assert!(cmd_rx.try_recv().is_err());
        assert!(gateway_rx.try_recv().is_err());
    }
    #[tokio::test]
    async fn cross_session_monitor_event_is_dropped() {
        let (config, mut gateway_rx, _persistence_rx, mut cmd_rx) = make_test_config_full();
        let notification = make_monitor_event_notification("mon-foreign", Some("other-session"));
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        assert!(
            cmd_rx.try_recv().is_err(),
            "cross-session monitor event must not be injected into this session"
        );
        while let Ok(msg) = gateway_rx.try_recv() {
            if let acp_transport::AcpClientMessage::ExtNotification(args) = msg {
                assert_ne!(
                    args.request.method.as_ref(),
                    "grow/monitor_event",
                    "cross-session monitor event must not be forwarded to the pager"
                );
            }
        }
    }
    #[tokio::test]
    async fn same_session_monitor_event_is_injected() {
        let (config, mut cmd_rx) = make_test_config();
        let notification = make_monitor_event_notification("mon-own", Some("test-session"));
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        match cmd_rx
            .try_recv()
            .expect("own-session monitor event must be injected")
        {
            SessionCommand::ReceiveNotification { source, body, .. } => {
                assert!(matches!(
                    source,
                    chat_state::NotificationSource::MonitorProgress { task_id, .. }
                        if task_id == "mon-own"
                ));
                assert!(body.contains("boom"));
            }
            _ => panic!("expected ReceiveNotification"),
        }
    }
    #[tokio::test]
    async fn monitor_event_without_owner_is_dropped() {
        let (config, mut cmd_rx) = make_test_config();
        let notification = make_monitor_event_notification("mon-legacy", None);
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        assert!(cmd_rx.try_recv().is_err());
    }
    #[tokio::test]
    async fn block_waited_task_skips_auto_wake_prompt() {
        let (config, mut gateway_rx, _persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut snapshot = make_task_snapshot("bg-waited", TaskKind::Bash);
        snapshot.block_waited = true;
        let notification = ToolNotification::TaskCompleted(snapshot);
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        match cmd_rx
            .try_recv()
            .expect("expected DispatchNotificationHook for task_complete")
        {
            SessionCommand::DispatchNotificationHook {
                notification_type, ..
            } => {
                assert_eq!(notification_type, "task_complete")
            }
            _ => panic!("unexpected session command"),
        }
        assert!(
            cmd_rx.try_recv().is_err(),
            "block_waited completion should not send a durable receipt"
        );
        let mut found_ext = false;
        while let Ok(msg) = gateway_rx.try_recv() {
            if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                && args.request.method.as_ref() == "grow/task_completed"
            {
                found_ext = true;
            }
        }
        assert!(
            found_ext,
            "grow/task_completed ExtNotification must still be sent for UI"
        );
    }
    #[tokio::test]
    async fn explicitly_killed_task_skips_auto_wake_prompt() {
        let (config, mut gateway_rx, _persistence_rx, mut cmd_rx) = make_test_config_full();
        let mut snapshot = make_task_snapshot("bg-killed", TaskKind::Bash);
        snapshot.explicitly_killed = true;
        let notification = ToolNotification::TaskCompleted(snapshot);
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        match cmd_rx
            .try_recv()
            .expect("expected DispatchNotificationHook for task_complete")
        {
            SessionCommand::DispatchNotificationHook {
                notification_type, ..
            } => {
                assert_eq!(notification_type, "task_complete")
            }
            _ => panic!("unexpected session command"),
        }
        assert!(
            cmd_rx.try_recv().is_err(),
            "explicitly_killed completion should not send a durable receipt"
        );
        let mut found_ext = false;
        while let Ok(msg) = gateway_rx.try_recv() {
            if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                && args.request.method.as_ref() == "grow/task_completed"
            {
                found_ext = true;
            }
        }
        assert!(
            found_ext,
            "grow/task_completed ExtNotification must still be sent for UI"
        );
    }
    #[tokio::test]
    async fn bash_completion_uses_single_task_id_clone() {
        let (config, mut cmd_rx) = make_test_config();
        let snapshot = make_task_snapshot("unique-id-789", TaskKind::Bash);
        let notification = ToolNotification::TaskCompleted(snapshot);
        let mut offsets = HashMap::new();
        handle_notification(&config, notification, &mut offsets).await;
        let cmd = cmd_rx.try_recv().unwrap();
        if let SessionCommand::ReceiveNotification { source, .. } = cmd {
            assert!(matches!(
                source,
                chat_state::NotificationSource::TaskCompleted { task_id, .. }
                    if task_id == "unique-id-789"
            ));
        } else {
            panic!("expected ReceiveNotification");
        }
    }
    /// Build a completed-bash `TaskSnapshot` whose `output` is large enough
    /// to trip the inline-completion truncation cap, with a concrete
    /// `output_file` path so the disk-pointer footer is exercised end-to-end.
    fn make_large_bash_snapshot(task_id: &str, output_file: PathBuf) -> TaskSnapshot {
        TaskSnapshot {
            goal_definition_revision: None,
            task_id: task_id.into(),
            command: "yes hello | head -c 20000".into(),
            display_command: None,
            cwd: String::new(),
            start_time: std::time::SystemTime::now(),
            end_time: Some(std::time::SystemTime::now()),
            output: "h".repeat(20_000),
            output_file,
            truncated: true,
            exit_code: Some(0),
            signal: None,
            completed: true,
            kind: TaskKind::Bash,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            description: None,
            is_backgrounded: false,
        }
    }
    /// Extract the durable notification body emitted on the session command channel.
    fn notification_body(cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>) -> String {
        let cmd = cmd_rx.try_recv().expect("expected ReceiveNotification");
        match cmd {
            SessionCommand::ReceiveNotification { body, .. } => body,
            _ => panic!("expected ReceiveNotification"),
        }
    }
    /// Bash completion with a large output and no task-output polling tool
    /// renders the truncation marker AND the disk-pointer footer
    /// pointing the model at `output_file` via the resolved Read tool name.
    #[tokio::test]
    async fn bash_completion_receipt_renders_disk_pointer_footer() {
        let output_file = PathBuf::from("/tmp/bg-disk-pointer.log");
        let (config_auto, mut cmd_rx_auto) = make_test_config();
        config_auto
            .read_tool_name
            .set(Some("read_file".to_string()))
            .expect("fresh slot");
        let snapshot = make_large_bash_snapshot("bg-disk-1", output_file.clone());
        let mut offsets = HashMap::new();
        handle_notification(
            &config_auto,
            ToolNotification::TaskCompleted(snapshot),
            &mut offsets,
        )
        .await;
        let prompt = notification_body(&mut cmd_rx_auto);
        assert!(
            prompt.contains("[Output truncated"),
            "expected truncation marker, got: {prompt}"
        );
        let expected_footer = format!(
            "Use read_file on {} for full content",
            output_file.display()
        );
        assert!(
            prompt.contains(&expected_footer),
            "expected disk-pointer footer `{expected_footer}`, got: {prompt}"
        );
        assert!(
            prompt.contains("bg-disk-1"),
            "receipt must reference task id"
        );
    }
}
