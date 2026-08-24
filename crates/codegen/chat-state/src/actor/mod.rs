//! ChatStateActor — runs in a dedicated tokio task and owns all chat state.
//!
//! This module is organized into submodules by responsibility:
//! - `state`: Internal state types (ChatState)
//! - `mutations`: State mutation handlers (push_user_message, replace_conversation, etc.)
//! - `queries`: Read-only query handlers (get_conversation, snapshot, etc.)

mod mutations;
mod queries;
pub(crate) mod request_builder;
pub mod state;

#[cfg(test)]
mod tests;

use tokio::sync::mpsc;
use tracing::debug;

use crate::commands::ChatStateCommand;
use crate::events::ChatStateEvent;
use crate::handle::ChatStateHandle;
use crate::persistence::TimelinePersistence;
use crate::types::{PruningConfig, TurnCapture};
use crate::{Timeline, TimelineEvent};

use sampling_types::{ConversationItem, SamplingConfig};
use state::ChatState;

/// The actor that owns all chat state.
/// Runs in a dedicated tokio task and processes commands sequentially.
pub struct ChatStateActor {
    /// Internal state — conversation, tokens, config, etc.
    state: ChatState,
    /// Pruning configuration for tool-result trimming.
    pruning_config: PruningConfig,
    /// Persistence implementation — owned exclusively, called with `&mut self`.
    persistence: Box<dyn TimelinePersistence>,
    /// Seed/recovery facts already present in `state`. The actor persists this
    /// prefix strictly one event at a time before admitting live commands.
    bootstrap_events: Vec<TimelineEvent>,
    /// Channel to receive commands from handles.
    cmd_rx: mpsc::UnboundedReceiver<ChatStateCommand>,
    /// Channel to send events to the session main loop.
    event_tx: mpsc::UnboundedSender<ChatStateEvent>,
    /// Cancellation token for graceful shutdown.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// A permanent persistence/integrity failure invalidates the actor's
    /// writer epoch. Continuing would allow a different event to reuse the
    /// unaccepted sequence, so the mailbox must close after the current
    /// command receives its failure.
    persistence_poisoned: bool,
}

impl ChatStateActor {
    /// Commit an already validated event before admitting it to actor state.
    /// The exact event bytes, including seq and timestamp, cross the durable
    /// boundary. Any failed or ambiguous acknowledgement keeps this exact
    /// event in the actor's single pending slot until durable success or
    /// session cancellation.
    async fn commit_timeline_event(
        &mut self,
        event: TimelineEvent,
    ) -> Result<TimelineEvent, crate::commands::TimelineWriteError> {
        self.persist_pending_timeline_event(&event).await?;
        let committed = event.clone();
        let previous_prompt_index = self.state.timeline.next_prompt_index();
        if let Err(error) = self.state.timeline.accept(event) {
            self.persistence_poisoned = true;
            return Err(crate::commands::TimelineWriteError::Invalid(error));
        }
        self.refresh_prompt_projection(previous_prompt_index);
        Ok(committed)
    }

    /// Persist a fire-and-forget command with actor-level backpressure. The
    /// same immutable event is retried until it commits or the actor is
    /// cancelled, so a transient ENOSPC/lock/I/O failure cannot create a
    /// missing sequence and poison every later append.
    async fn persist_pending_timeline_event(
        &mut self,
        event: &TimelineEvent,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let mut retry_delay = std::time::Duration::from_millis(25);
        loop {
            let acknowledgement = self.persistence.persist_timeline_event_and_ack(event);
            let result = tokio::select! {
                biased;
                _ = self.cancellation_token.cancelled() => {
                    return Err(crate::commands::TimelineWriteError::Cancelled);
                }
                result = acknowledgement => result,
            };
            let result = match result {
                Ok(result) => result.map_err(crate::commands::TimelineWriteError::Persistence),
                Err(_) => Err(crate::commands::TimelineWriteError::AcknowledgementLost),
            };
            match result {
                Ok(()) => return Ok(()),
                Err(crate::commands::TimelineWriteError::Persistence(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::InvalidData
                            | std::io::ErrorKind::InvalidInput
                            | std::io::ErrorKind::PermissionDenied
                            | std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::Unsupported
                            | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    tracing::error!(
                        %error,
                        seq = event.seq.get(),
                        "Timeline persistence entered a permanent failed state"
                    );
                    self.persistence_poisoned = true;
                    return Err(crate::commands::TimelineWriteError::Persistence(error));
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        seq = event.seq.get(),
                        retry_delay_ms = retry_delay.as_millis(),
                        "buffered Timeline commit failed; applying backpressure and retrying exact event"
                    );
                }
            }
            tokio::select! {
                biased;
                _ = self.cancellation_token.cancelled() => {
                    return Err(crate::commands::TimelineWriteError::Cancelled);
                }
                _ = tokio::time::sleep(retry_delay) => {}
            }
            retry_delay = retry_delay
                .saturating_mul(2)
                .min(std::time::Duration::from_secs(1));
        }
    }

    async fn commit_buffered_timeline_event(&mut self, event: TimelineEvent) -> bool {
        if self.persist_pending_timeline_event(&event).await.is_err() {
            return false;
        }
        let previous_prompt_index = self.state.timeline.next_prompt_index();
        self.state.timeline.accept(event).unwrap_or_else(|error| {
            self.persistence_poisoned = true;
            tracing::error!(%error, "persisted Timeline event could not be accepted");
        });
        if self.persistence_poisoned {
            return false;
        }
        self.refresh_prompt_projection(previous_prompt_index);
        true
    }

    fn refresh_prompt_projection(&mut self, previous_prompt_index: usize) {
        let next_prompt_index = self.state.timeline.next_prompt_index();
        if next_prompt_index != previous_prompt_index {
            self.state.prompt_usage = None;
            self.send_event(ChatStateEvent::PromptIndexChanged {
                new_index: next_prompt_index,
            });
        }
    }

    /// Send an event to subscribers, logging if the channel is closed.
    fn send_event(&self, event: ChatStateEvent) {
        if self.event_tx.send(event).is_err() {
            debug!("ChatState event channel closed, event dropped");
        }
    }

    /// Spawn the actor and return a handle to communicate with it.
    pub fn spawn(
        initial_conversation: Vec<ConversationItem>,
        sampling_config: SamplingConfig,
        persistence: Box<dyn TimelinePersistence>,
        event_tx: mpsc::UnboundedSender<ChatStateEvent>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> ChatStateHandle {
        Self::spawn_with_pruning(
            initial_conversation,
            sampling_config,
            PruningConfig::default(),
            persistence,
            event_tx,
            cancellation_token,
        )
    }

    /// Spawn the actor with a custom pruning config.
    pub fn spawn_with_pruning(
        initial_conversation: Vec<ConversationItem>,
        sampling_config: SamplingConfig,
        pruning_config: PruningConfig,
        persistence: Box<dyn TimelinePersistence>,
        event_tx: mpsc::UnboundedSender<ChatStateEvent>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> ChatStateHandle {
        let state = ChatState::new(initial_conversation, sampling_config);
        let bootstrap_events = state.timeline.events().to_vec();
        Self::launch(
            state,
            pruning_config,
            persistence,
            bootstrap_events,
            event_tx,
            cancellation_token,
        )
    }

    /// Restore an actor from its durable append-only event stream.
    pub async fn spawn_from_timeline_with_pruning(
        timeline_events: Vec<TimelineEvent>,
        sampling_config: SamplingConfig,
        pruning_config: PruningConfig,
        persistence: Box<dyn TimelinePersistence>,
        event_tx: mpsc::UnboundedSender<ChatStateEvent>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<ChatStateHandle, crate::commands::TimelineWriteError> {
        let timeline = Timeline::from_events(timeline_events)?;
        Self::spawn_from_validated_timeline_with_pruning(
            timeline,
            sampling_config,
            pruning_config,
            persistence,
            event_tx,
            cancellation_token,
        )
        .await
    }

    /// Restore an actor from an already validated Timeline. This is the
    /// canonical handoff for callers that need the Surface during bootstrap:
    /// validation/materialization happens once, then ownership moves here.
    pub async fn spawn_from_validated_timeline_with_pruning(
        mut timeline: Timeline,
        sampling_config: SamplingConfig,
        pruning_config: PruningConfig,
        persistence: Box<dyn TimelinePersistence>,
        event_tx: mpsc::UnboundedSender<ChatStateEvent>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<ChatStateHandle, crate::commands::TimelineWriteError> {
        let mut recovery_events = timeline.recover_interrupted()?;
        recovery_events.extend(timeline.recover_surface_integrity()?);
        let state = ChatState::from_timeline(timeline, sampling_config);
        Ok(Self::launch(
            state,
            pruning_config,
            persistence,
            recovery_events,
            event_tx,
            cancellation_token,
        ))
    }

    fn launch(
        state: ChatState,
        pruning_config: PruningConfig,
        persistence: Box<dyn TimelinePersistence>,
        bootstrap_events: Vec<TimelineEvent>,
        event_tx: mpsc::UnboundedSender<ChatStateEvent>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> ChatStateHandle {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let actor_cancellation = cancellation_token.child_token();
        let actor = ChatStateActor {
            state,
            pruning_config,
            persistence,
            bootstrap_events,
            cmd_rx,
            event_tx,
            cancellation_token: actor_cancellation.clone(),
            persistence_poisoned: false,
        };

        tokio::spawn(actor.run());

        ChatStateHandle::new(cmd_tx, actor_cancellation)
    }

    /// Main actor loop — processes commands until shutdown or cancellation.
    async fn run(mut self) {
        for event in std::mem::take(&mut self.bootstrap_events) {
            if self.persist_pending_timeline_event(&event).await.is_err() {
                debug!("ChatStateActor cancelled before bootstrap Timeline became durable");
                return;
            }
        }
        loop {
            tokio::select! {
                biased;
                _ = self.cancellation_token.cancelled() => {
                    debug!("ChatStateActor shutting down via cancellation");
                    break;
                }
                cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = cmd else {
                        debug!("ChatStateActor shutting down: all handles dropped");
                        break;
                    };
                    self.handle_command(cmd).await;
                    if self.persistence_poisoned {
                        tracing::error!(
                            "ChatStateActor writer epoch poisoned; closing mailbox"
                        );
                        break;
                    }
                }
            }
        }
    }

    /// Dispatch a command to the appropriate mutation or query handler.
    async fn handle_command(&mut self, cmd: ChatStateCommand) {
        match cmd {
            // ═══ Mutations ═══
            ChatStateCommand::PushUserMessage { item } => {
                self.push_user_message(item).await;
            }
            ChatStateCommand::RecordTimelineEvent { kind } => {
                match self.state.timeline.prepare(kind) {
                    Ok(event) => {
                        self.commit_buffered_timeline_event(event).await;
                    }
                    Err(error) => tracing::error!(%error, "rejected invalid timeline event"),
                }
            }
            ChatStateCommand::RecordTimelineEventDurably { kind, reply } => {
                let result = match self.state.timeline.prepare(kind) {
                    Err(error) => Err(crate::commands::TimelineWriteError::Invalid(error)),
                    Ok(event) => self.commit_timeline_event(event).await,
                };
                let _ = reply.send(result);
            }
            ChatStateCommand::RecoverInterruptedDurably { reply } => {
                let result = match (|| {
                    let mut candidate = self.state.timeline.clone();
                    candidate.recover_interrupted()
                })() {
                    Err(error) => Err(crate::commands::TimelineWriteError::Invalid(error)),
                    Ok(events) => {
                        let mut committed = Vec::with_capacity(events.len());
                        let mut error = None;
                        for event in events {
                            match self.commit_timeline_event(event).await {
                                Ok(event) => committed.push(event),
                                Err(write_error) => {
                                    error = Some(write_error);
                                    break;
                                }
                            }
                        }
                        error.map_or(Ok(committed), Err)
                    }
                };
                let _ = reply.send(result);
            }
            ChatStateCommand::PushUserMessageDurably { item, reply } => {
                let result = self.push_user_message_durably(item).await;
                let _ = reply.send(result);
            }
            ChatStateCommand::PushUserMessageWithRepairReason { item, reason } => {
                self.push_user_message_with_repair_reason(item, reason)
                    .await;
            }
            ChatStateCommand::PushAssistantResponse { item } => {
                self.push_message(item).await;
            }
            ChatStateCommand::PushToolResult { item } => {
                self.push_message(item).await;
            }
            ChatStateCommand::PushToolResultConditionally {
                item,
                rejection_item,
                expected_surface_revision,
                max_estimated_total_tokens,
                max_result_tokens,
                reply,
            } => {
                let result = self
                    .push_tool_result_conditionally(
                        item,
                        rejection_item,
                        expected_surface_revision,
                        max_estimated_total_tokens,
                        max_result_tokens,
                    )
                    .await;
                let _ = reply.send(result);
            }
            ChatStateCommand::RecordTokenUsage { total_tokens } => {
                self.record_token_usage(total_tokens);
            }
            ChatStateCommand::RecordLastTurnUsage { usage } => {
                self.record_last_turn_usage(usage);
            }
            ChatStateCommand::RecordModelCallUsage {
                model_id,
                usage,
                api_duration_ms,
                cost_usd_ticks,
            } => {
                self.record_model_call_usage(model_id, &usage, api_duration_ms, cost_usd_ticks);
            }
            ChatStateCommand::RecordSubagentUsage {
                by_model,
                attribute_to_prompt,
                incomplete,
                reply,
            } => {
                self.record_subagent_usage(&by_model, attribute_to_prompt, incomplete);
                let _ = reply.send(());
            }
            ChatStateCommand::MarkUsageIncomplete {
                prompt,
                session,
                reply,
            } => {
                self.mark_usage_incomplete(prompt, session);
                let _ = reply.send(());
            }
            ChatStateCommand::UpdateSamplingConfig { config } => {
                self.state.sampling_config = config;
            }
            ChatStateCommand::RecordAgentEditedPath { path } => {
                self.state.agent_edited_paths.insert(path);
            }
            ChatStateCommand::RecordStreamStart { timestamp_ms } => {
                self.state.stream_start_ms = Some(timestamp_ms);
            }
            ChatStateCommand::RecordTurnStart { timestamp_ms } => {
                self.state.turn_start_ms = Some(timestamp_ms);
            }
            ChatStateCommand::ReplaceSurfaceDurably {
                items,
                cause,
                expected_surface_revision,
                reply,
            } => {
                let actual = self.state.timeline.surface_revision();
                let result = if actual != expected_surface_revision {
                    Err(crate::commands::TimelineWriteError::SurfaceChanged {
                        expected: expected_surface_revision,
                        actual,
                    })
                } else {
                    self.replace_conversation_durably(items, cause).await
                };
                let _ = reply.send(result);
            }
            ChatStateCommand::ReplaceCompactionRangeDurably {
                target,
                items,
                expected_surface_revision,
                reply,
            } => {
                let actual = self.state.timeline.surface_revision();
                let result = if actual != expected_surface_revision {
                    Err(crate::commands::TimelineWriteError::SurfaceChanged {
                        expected: expected_surface_revision,
                        actual,
                    })
                } else {
                    self.replace_compaction_range_durably(target, items).await
                };
                let _ = reply.send(result);
            }
            ChatStateCommand::SeedTokenAccounting {
                total_tokens,
                reply,
            } => {
                self.seed_token_accounting(total_tokens);
                let _ = reply.send(());
            }
            ChatStateCommand::RewindDurably {
                target_prompt_index,
                reply,
            } => {
                let result = self.rewind_durably(target_prompt_index).await;
                let _ = reply.send(result);
            }
            ChatStateCommand::PruneToolResults { plan, reply } => {
                let report = self.prune_tool_results(plan).await;
                let _ = reply.send(report);
            }
            ChatStateCommand::RewriteImagesAndAck {
                rewrites,
                dropped_placeholder,
                reply,
            } => {
                let report = self.rewrite_images(rewrites, &dropped_placeholder).await;
                let _ = reply.send(report);
            }
            ChatStateCommand::RepairHistory {
                dry_run,
                turn_active,
                reply,
            } => {
                // Checked here so refusal and mutation are serialized; a
                // `false` at processing time means pre-turn state (see the
                // command's doc).
                let blocked = turn_active
                    .as_ref()
                    .map(|f| f.load(std::sync::atomic::Ordering::SeqCst))
                    .unwrap_or(false);
                let result = if blocked {
                    Err(crate::commands::RepairHistoryError::TurnActive)
                } else {
                    self.repair_history(dry_run)
                        .await
                        .map_err(crate::commands::RepairHistoryError::Timeline)
                };
                let _ = reply.send(result);
            }
            ChatStateCommand::Flush => {
                self.persistence.flush();
            }
            ChatStateCommand::UpdateCredentials { credentials } => {
                self.state.credentials = credentials;
            }
            ChatStateCommand::BeginTurnCapture => {
                self.state.turn_capture = Some(state::TurnCaptureState {
                    turn_start_seq: self.state.timeline.next_seq(),
                    compaction_occurred: false,
                });
            }
            ChatStateCommand::RepairDanglingAfterHarnessHalt { class } => {
                self.repair_dangling_after_harness_halt(class).await;
            }

            // ═══ Queries ═══
            //
            // Read queries are pure reads — repair only at write boundaries:
            // `ChatState::new()` (startup) and `push_user_message()` (new turn).
            // `BuildConversationRequest` retains the guard because it is only
            // ever issued by the agent loop between turns, never by background tasks.
            ChatStateCommand::BuildConversationRequest {
                timeline_id,
                tool_definitions,
                memory_reminder,
                reply,
            } => {
                let result = self
                    .build_conversation_request(
                        &timeline_id,
                        tool_definitions,
                        memory_reminder,
                    )
                    .await;
                let _ = reply.send(result);
            }
            ChatStateCommand::GetConversation { reply } => {
                tracing::debug!(
                    conversation_len = self.state.timeline.surface_len(),
                    "ChatState: cloning full conversation for GetConversation"
                );
                let _ = reply.send(self.state.timeline.surface().to_vec());
            }
            ChatStateCommand::GetConversationWithRevision { reply } => {
                let _ = reply.send((
                    self.state.timeline.surface().to_vec(),
                    self.state.timeline.surface_revision(),
                ));
            }
            ChatStateCommand::GetTrajectory { reply } => {
                let _ = reply.send(self.state.timeline.trajectory());
            }
            ChatStateCommand::GetTimelineEvents { reply } => {
                let _ = reply.send(self.state.timeline.events().to_vec());
            }
            ChatStateCommand::MaterializeTimeline { timeline_id, reply } => {
                let materialized = self.state.timeline.events().last().map(|event| {
                    crate::TimelineMaterialization {
                        input_ref: crate::TimelineRangeRef {
                            timeline_id,
                            first_seq: 0,
                            last_seq: event.seq.get(),
                        },
                        surface_revision: self.state.timeline.surface_revision(),
                        surface: self.state.timeline.surface().to_vec(),
                        surface_ids: self.state.timeline.surface_ids().to_vec(),
                        active_control_contexts: self.state.timeline.active_control_contexts(),
                    }
                });
                let _ = reply.send(materialized);
            }
            ChatStateCommand::MaterializeBranchTranscript { timeline_id, reply } => {
                let materialized = self.state.timeline.events().last().map(|event| {
                    let (transcript_ids, transcript) =
                        self.state.timeline.branch_transcript_with_ids();
                    crate::RecallMaterialization {
                        source_ref: crate::TimelineRangeRef {
                            timeline_id,
                            first_seq: 0,
                            last_seq: event.seq.get(),
                        },
                        surface_revision: self.state.timeline.surface_revision(),
                        need_surface_ids: self.state.timeline.surface_ids().to_vec(),
                        transcript,
                        transcript_ids,
                        unloaded_surface_ids: self
                            .state
                            .timeline
                            .completed_compaction_unloaded_branch_ids(),
                    }
                });
                let _ = reply.send(materialized);
            }
            ChatStateCommand::GetPromptIndex { reply } => {
                let _ = reply.send(self.state.timeline.next_prompt_index());
            }
            ChatStateCommand::GetSurfaceRevision { reply } => {
                let _ = reply.send(self.state.timeline.surface_revision());
            }
            ChatStateCommand::GetLastCompactionPromptIndex { reply } => {
                let _ = reply.send(self.state.timeline.last_completed_compaction_prompt_index());
            }
            ChatStateCommand::GetTotalTokens { reply } => {
                let _ = reply.send(self.state.total_tokens);
            }
            ChatStateCommand::GetLastTurnUsage { reply } => {
                let _ = reply.send(self.state.last_turn_usage.clone());
            }
            ChatStateCommand::GetPromptUsage { reply } => {
                let _ = reply.send(self.state.prompt_usage.clone());
            }
            ChatStateCommand::GetSessionUsage { reply } => {
                let _ = reply.send(self.state.session_usage.clone());
            }
            ChatStateCommand::GetEstimatedTotalTokens { reply } => {
                let _ =
                    reply.send(self.state.total_tokens + self.state.estimated_tokens_since_model);
            }
            ChatStateCommand::GetSamplingConfig { reply } => {
                let _ = reply.send(self.state.sampling_config.clone());
            }
            ChatStateCommand::GetAgentEditedPaths { reply } => {
                let _ = reply.send(self.state.agent_edited_paths.clone());
            }
            ChatStateCommand::GetNotificationMeta { reply } => {
                let _ = reply.send(self.get_notification_meta());
            }
            ChatStateCommand::Snapshot { reply } => {
                tracing::debug!(
                    conversation_len = self.state.timeline.surface_len(),
                    "ChatState: cloning full state for Snapshot"
                );
                let _ = reply.send(self.snapshot());
            }
            ChatStateCommand::CheckAutoCompactNeeded {
                threshold_percent,
                reply,
            } => {
                let _ = reply.send(self.check_auto_compact_needed(threshold_percent));
            }
            ChatStateCommand::GetCredentials { reply } => {
                let _ = reply.send(self.state.credentials.clone());
            }
            ChatStateCommand::GetLastModelMetadata { reply } => {
                let _ = reply.send(self.get_last_model_metadata());
            }
            ChatStateCommand::TakeTurnMessages { reply } => {
                let result = self.state.turn_capture.take().map(|cap| TurnCapture {
                    messages: self.state.timeline.turn_items_since(cap.turn_start_seq),
                    compaction_occurred: cap.compaction_occurred,
                });
                let _ = reply.send(result);
            }
            // ─── Narrow targeted queries ──────────────────────────────────
            ChatStateCommand::GetConversationLen { reply } => {
                let _ = reply.send(self.get_conversation_len());
            }
            ChatStateCommand::HasDanglingToolCalls { reply } => {
                let _ = reply.send(self.has_dangling_tool_calls());
            }
            ChatStateCommand::GetLastAssistantText { reply } => {
                let _ = reply.send(self.get_last_assistant_text());
            }
            ChatStateCommand::GetLastAssistantTextInTurn { reply } => {
                let _ = reply.send(self.get_last_assistant_text_in_turn());
            }
            ChatStateCommand::GetFirstUserText { reply } => {
                let _ = reply.send(self.get_first_user_text());
            }
            ChatStateCommand::GetConversationItemAt { index, reply } => {
                let _ = reply.send(self.get_conversation_item_at(index));
            }
            ChatStateCommand::GetLastUserQueryText { reply } => {
                let _ = reply.send(self.get_last_user_query_text());
            }
            ChatStateCommand::GetConversationCounts { reply } => {
                let _ = reply.send(self.get_conversation_counts());
            }
            ChatStateCommand::GetSystemMessage { reply } => {
                let _ = reply.send(self.get_system_message());
            }
            ChatStateCommand::GetEstimatedMessagesTokens { reply } => {
                let _ = reply.send(state::estimate_messages_tokens(
                    self.state.timeline.surface(),
                ));
            }
        }
    }
}
