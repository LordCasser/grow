//! Durable human-input admission and recovery helpers.

use super::*;

pub(super) struct AdmittedInput {
    pub input_id: String,
}

impl SessionActor {
    pub(super) async fn admit_human_input(
        &self,
        intent: chat_state::InputIntent,
        payload: crate::session::input_inbox::InputPayload,
        prompt_id: Option<String>,
        route: chat_state::InputRoute,
        supersedes: Vec<String>,
    ) -> Result<AdmittedInput, String> {
        let (input_id, decision) = self
            .run_human_input_admission_hook(intent, payload, prompt_id)
            .await?;
        let (route, supersedes) = if matches!(decision, chat_state::InputAdmissionDecision::Allow) {
            (Some(route), supersedes)
        } else {
            (None, Vec::new())
        };
        self.resolve_human_input_admission(input_id, decision, route, supersedes)
            .await
    }

    /// Publish a HumanIntent and run its one UserPromptSubmit Hook occurrence,
    /// but leave the final admission decision to the caller. Direct steering
    /// uses this split phase so target liveness is checked after a potentially
    /// slow Hook without losing the submitted input fact.
    pub(super) async fn run_human_input_admission_hook(
        &self,
        intent: chat_state::InputIntent,
        payload: crate::session::input_inbox::InputPayload,
        prompt_id: Option<String>,
    ) -> Result<(String, chat_state::InputAdmissionDecision), String> {
        let input_id = format!("input-{}", uuid::Uuid::now_v7());
        let hook_prompt = payload.hook_prompt();
        let artifact_guard = self.input_artifact_gate.lock().await;
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
        let payload_ref = tokio::task::spawn_blocking(move || {
            crate::session::input_inbox::write_payload(&directory, &payload)
        })
        .await
        .map_err(|error| format!("input artifact writer failed: {error}"))?
        .map_err(|error| format!("input artifact write failed: {error}"))?;

        self.chat_state_handle
            .submit_input_durably(input_id.clone(), intent, payload_ref.clone())
            .await
            .map_err(|error| format!("input submission was not durable: {error}"))?;
        // The blob is now reachable from Timeline, so maintenance can no longer
        // classify it as an orphan. Do not serialize slow external Hook runs on
        // the artifact-maintenance gate.
        drop(artifact_guard);

        let envelope = self.make_hook_envelope(
            ::hooks::event::HookEventName::UserPromptSubmit,
            prompt_id,
            ::hooks::event::HookPayload::UserPromptSubmit {
                prompt: Some(hook_prompt),
            },
        );
        let aggregate = self
            .dispatch_prompt_hook(
                chat_state::HookCause::Input {
                    input_id: input_id.clone(),
                },
                envelope,
                ::hooks::event::GateKind::Prompt,
            )
            .await
            .map_err(|error| format!("input hook lifecycle was not durable: {error}"))?;
        let decision = match aggregate {
            HookAggregate::Prompt { decision, .. } => decision,
            _ => unreachable!("Prompt gate returned a non-Prompt aggregate"),
        };
        let decision = match decision {
            ::hooks::result::HookDecision::Allow => chat_state::InputAdmissionDecision::Allow,
            ::hooks::result::HookDecision::Deny { reason, .. } => {
                chat_state::InputAdmissionDecision::Block {
                    reason: chat_state::InputBlockReason::Hook {
                        reason: reason.clone(),
                    },
                }
            }
        };
        Ok((input_id, decision))
    }

    pub(super) async fn resolve_human_input_admission(
        &self,
        input_id: String,
        decision: chat_state::InputAdmissionDecision,
        route: Option<chat_state::InputRoute>,
        supersedes: Vec<String>,
    ) -> Result<AdmittedInput, String> {
        self.record_input_event(chat_state::InputEvent::AdmissionResolved {
            input_id: input_id.clone(),
            decision: decision.clone(),
            route,
            supersedes,
        })
        .await?;
        if let chat_state::InputAdmissionDecision::Block { reason } = decision {
            return Err(match reason {
                chat_state::InputBlockReason::Hook { reason } => reason,
                chat_state::InputBlockReason::StaleSteerTarget => {
                    "the target turn is no longer running".to_string()
                }
                chat_state::InputBlockReason::ProcessInterrupted => {
                    "input admission was interrupted".to_string()
                }
            });
        }
        Ok(AdmittedInput { input_id })
    }

    pub(super) async fn reroute_input(
        &self,
        input_id: String,
        route: chat_state::InputRoute,
    ) -> Result<(), String> {
        self.reroute_input_ids(vec![input_id], route).await
    }

    pub(super) async fn reroute_input_ids(
        &self,
        input_ids: Vec<String>,
        route: chat_state::InputRoute,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Rerouted { input_ids, route })
            .await
    }

    pub(super) async fn consume_steer_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
        item: sampling_types::ConversationItem,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
        })
        .await
    }

    pub(super) async fn consume_fifo_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
        item: sampling_types::ConversationItem,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
        })
        .await
    }

    pub(super) async fn complete_unmodeled_fifo_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Handled { input_ids, turn })
            .await
    }

    pub(super) async fn dismiss_input_ids(
        &self,
        input_ids: impl IntoIterator<Item = String>,
        reason: chat_state::InputDismissReason,
    ) -> Result<(), String> {
        let input_ids = input_ids.into_iter().collect::<Vec<_>>();
        if input_ids.is_empty() {
            return Ok(());
        }
        self.record_input_event(chat_state::InputEvent::Dismissed { input_ids, reason })
            .await
    }

    async fn record_input_event(&self, event: chat_state::InputEvent) -> Result<(), String> {
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Input(event))
            .await
            .map(|_| ())
            .map_err(|error| format!("input lifecycle was not durable: {error}"))
    }

    pub(super) async fn restore_pending_human_inputs(&self) -> Result<(), String> {
        let pending = self
            .chat_state_handle
            .get_pending_allowed_inputs()
            .await
            .ok_or_else(|| "Timeline unavailable while restoring pending inputs".to_string())?;
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
        for pending in pending {
            let directory = directory
                .try_clone()
                .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
            let payload_ref = pending.payload_ref.clone();
            let payload = tokio::task::spawn_blocking(move || {
                crate::session::input_inbox::read_payload(&directory, &payload_ref)
            })
            .await
            .map_err(|error| format!("input artifact reader failed: {error}"))?
            .map_err(|error| format!("input payload is missing or corrupt: {error}"))?;
            if !crate::session::input_inbox::payload_matches_intent(pending.intent, &payload) {
                return Err("input intent does not match its payload artifact".into());
            }
            match (pending.intent, payload) {
                (
                    chat_state::InputIntent::Prompt
                    | chat_state::InputIntent::Followup
                    | chat_state::InputIntent::Steer,
                    crate::session::input_inbox::InputPayload::Prompt {
                        prompt_id,
                        prompt_blocks,
                        client_identifier,
                        screen_mode,
                        verbatim,
                        json_schema,
                        origin,
                        turn_kind,
                        queue,
                    },
                ) => {
                    if origin != crate::session::PromptOrigin::User
                        || turn_kind != crate::session::TurnKind::User
                    {
                        return Err("pending human input payload has a synthetic identity".into());
                    }
                    if matches!(pending.route, chat_state::InputRoute::Steer { .. }) {
                        self.reroute_input(pending.input_id.clone(), chat_state::InputRoute::Fifo)
                            .await?;
                    } else if !matches!(pending.route, chat_state::InputRoute::Fifo) {
                        return Err("pending prompt input has an incompatible route".into());
                    }
                    let queue_meta = queue.map(Into::into).unwrap_or_else(|| {
                        crate::session::prompt_queue::QueueEntryMeta {
                            id: prompt_id.clone(),
                            version: 0,
                            owner: client_identifier.clone(),
                            last_editor: None,
                            kind: "prompt".into(),
                            text: Self::queue_text_from_blocks(&prompt_blocks),
                            combined_texts: None,
                        }
                    });
                    let (respond_to, _response) = tokio::sync::oneshot::channel();
                    self.state.lock().await.pending_inputs.push_back(InputItem {
                        input_ids: vec![pending.input_id],
                        prompt_id,
                        turn_kind,
                        prompt_blocks,
                        client_identifier,
                        screen_mode,
                        verbatim,
                        json_schema,
                        origin,
                        host_command: None,
                        notification_ids: Vec::new(),
                        respond_to,
                        persist_ack: None,
                        queue_meta: Some(queue_meta),
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) async fn reconcile_input_payloads(
        &self,
        shutdown: &tokio_util::sync::CancellationToken,
    ) {
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::warn!(%error, "input payload reconciliation directory unavailable");
                return;
            }
        };
        let (batch_tx, mut batch_rx) = tokio::sync::mpsc::channel::<Vec<String>>(1);
        let producer_shutdown = shutdown.clone();
        let producer = tokio::task::spawn_blocking(move || {
            crate::session::input_inbox::visit_payload_hash_batches(
                &directory,
                || batch_tx.is_closed() || producer_shutdown.is_cancelled(),
                |batch| {
                    batch_tx.blocking_send(batch).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "input payload reconciler stopped",
                        )
                    })
                },
            )
        });
        while let Some(mut hashes) = tokio::select! {
            biased;
            _ = shutdown.cancelled() => None,
            hashes = batch_rx.recv() => hashes,
        } {
            let _guard = self.input_artifact_gate.lock().await;
            let Some(retained) = self
                .chat_state_handle
                .submitted_input_payload_hashes()
                .await
            else {
                break;
            };
            // Submitted payloads remain part of the immutable audit record even
            // after the input is blocked, consumed, or dismissed. Only blobs with
            // no Timeline reference are orphans.
            hashes.retain(|hash| !retained.contains(hash));
            if hashes.is_empty() {
                continue;
            }
            let Ok(directory) = self.session_directory.try_clone() else {
                break;
            };
            if tokio::task::spawn_blocking(move || {
                crate::session::input_inbox::remove_payload_hashes(&directory, &hashes)
            })
            .await
            .is_err()
            {
                break;
            }
        }
        drop(batch_rx);
        let _ = producer.await;
    }
}
