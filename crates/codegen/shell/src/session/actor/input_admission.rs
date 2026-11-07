//! Durable human-input admission and recovery helpers.

use super::*;

fn comparable_input_payload(
    payload: &crate::session::input_inbox::InputPayload,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(payload)
        .map_err(|error| format!("input payload cannot be compared: {error}"))?;
    if let Some(object) = value.as_object_mut() {
        object.remove("skill_invocations");
    }
    Ok(value)
}

#[derive(Debug)]
pub(super) struct AdmittedInput {
    pub input_id: String,
    /// The attempt hit the canonical lifecycle of an already-recorded
    /// immutable request. This is an observation of the durable identity, not
    /// a fresh admission; no Hook, queue entry, or execution may be repeated.
    pub replayed: bool,
    /// Canonical state of the recorded input when `replayed` is true.
    pub replay_state: Option<chat_state::InputState>,
}

pub(super) enum InputAdmissionAttempt {
    New {
        input_id: String,
        decision: chat_state::InputAdmissionDecision,
    },
    Existing {
        input_id: String,
        state: chat_state::InputState,
    },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplicitSkillSelection {
    name: String,
    path: String,
    plugin_name: Option<String>,
}

fn explicit_selection_matches(
    selection: &ExplicitSkillSelection,
    reference: &crate::session::slash_commands::ParsedSkillRef,
) -> bool {
    reference.name == selection.name
        && reference.skill_path == selection.path
        && reference.plugin_name == selection.plugin_name
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
        let _admission_guard = self.input_admission_gate.lock().await;
        let attempt = self
            .run_human_input_admission_hook(intent, payload, prompt_id.clone())
            .await?;
        let (input_id, decision) = match attempt {
            InputAdmissionAttempt::New { input_id, decision } => (input_id, decision),
            InputAdmissionAttempt::Existing { input_id, state } => {
                // A retry of an already-recorded immutable request is an
                // observation of the canonical lifecycle, not a fresh
                // admission: no Hook, queue entry, or execution is repeated,
                // regardless of the recorded state.
                return Ok(AdmittedInput {
                    input_id,
                    replayed: true,
                    replay_state: Some(state),
                });
            }
        };
        let (route, supersedes) = if matches!(decision, chat_state::InputAdmissionDecision::Allow) {
            (Some(route), supersedes)
        } else {
            (None, Vec::new())
        };
        let admitted = self
            .resolve_human_input_admission(input_id, decision, route, supersedes)
            .await?;
        Ok(AdmittedInput {
            replayed: false,
            ..admitted
        })
    }

    fn explicit_skill_selection(
        prompt_blocks: &[acp::ContentBlock],
    ) -> Result<Option<ExplicitSkillSelection>, String> {
        let mut selected = None;
        for block in prompt_blocks {
            let acp::ContentBlock::Text(text) = block else {
                continue;
            };
            let Some(value) = text
                .meta
                .as_ref()
                .and_then(|meta| meta.get("grow/skillSelection"))
            else {
                continue;
            };
            if selected.is_some() {
                return Err("input contains multiple explicit skill selections".into());
            }
            let selection: ExplicitSkillSelection = serde_json::from_value(value.clone())
                .map_err(|_| "explicit skill selection metadata is malformed".to_string())?;
            if selection.name.trim().is_empty() || selection.path.trim().is_empty() {
                return Err("explicit skill selection identity is incomplete".into());
            }
            selected = Some(selection);
        }
        Ok(selected)
    }

    async fn freeze_skill_invocations(
        &self,
        prompt_blocks: &[acp::ContentBlock],
    ) -> Vec<crate::session::input_inbox::FrozenSkillInvocation> {
        use crate::session::input_inbox::FrozenSkillInvocation;
        use crate::session::slash_commands::SlashCommandOutcome;
        use tools::implementations::skills::skill::format_skill_name;

        let selection = match Self::explicit_skill_selection(prompt_blocks) {
            Ok(selection) => selection,
            Err(error) => {
                return vec![FrozenSkillInvocation {
                    callable_name: String::new(),
                    qualified_name: String::new(),
                    args: String::new(),
                    skill: None,
                    selection_error: Some(error),
                }];
            }
        };
        let skills = self.slash_skills_for_resolve().await;
        // Admission may already own the queue-edit control fence. Resolving
        // immutable user intent must not reconcile Goal or mutate business state.
        let tool_names = self.registered_tool_names().await;
        let has_workflow_runs = !self.workflow_tracker().await.lock().list().is_empty();
        let availability = self.build_command_availability(&tool_names, has_workflow_runs);
        let (_, workflows, _) = self.named_workflow_snapshot();
        let parsed = match crate::session::slash_commands::resolve(
            prompt_blocks.to_vec(),
            &skills,
            availability,
            &workflows,
        ) {
            Err(SlashCommandOutcome::InvokeSkill { skills, .. }) => skills,
            Ok(_) | Err(SlashCommandOutcome::Builtin(_)) => Vec::new(),
        };

        if let Some(selection) = selection {
            let selected = parsed
                .first()
                .filter(|reference| explicit_selection_matches(&selection, reference));
            if selected.is_none() {
                return vec![FrozenSkillInvocation {
                    callable_name: selection.name.clone(),
                    qualified_name: selection.name,
                    args: parsed
                        .first()
                        .map_or_else(String::new, |reference| reference.args.clone()),
                    skill: None,
                    selection_error: Some(
                        "selected skill source no longer matches this command; choose it again"
                            .into(),
                    ),
                }];
            }
        }

        parsed
            .into_iter()
            .map(|reference| {
                let skill = skills.iter().find(|skill| {
                    skill.path == reference.skill_path
                        && skill.plugin_name == reference.plugin_name
                        && format_skill_name(skill) == reference.qualified_name
                });
                match skill {
                    Some(skill) => FrozenSkillInvocation {
                        callable_name: reference.name,
                        qualified_name: reference.qualified_name,
                        args: reference.args,
                        // Keep the exact in-memory list snapshot. Admission
                        // does not reread SKILL.md or refresh its body.
                        skill: Some(skill.clone()),
                        selection_error: None,
                    },
                    None => FrozenSkillInvocation {
                        callable_name: reference.name,
                        qualified_name: reference.qualified_name,
                        args: reference.args,
                        skill: None,
                        selection_error: Some(
                            "resolved skill source disappeared before input admission".into(),
                        ),
                    },
                }
            })
            .collect()
    }

    /// Publish a HumanIntent and run its one UserPromptSubmit Hook occurrence,
    /// but leave the final admission decision to the caller. Direct steering
    /// uses this split phase so target liveness is checked after a potentially
    /// slow Hook without losing the submitted input fact.
    pub(super) async fn run_human_input_admission_hook(
        &self,
        intent: chat_state::InputIntent,
        mut payload: crate::session::input_inbox::InputPayload,
        prompt_id: Option<String>,
    ) -> Result<InputAdmissionAttempt, String> {
        let payload_prompt_id = match &payload {
            crate::session::input_inbox::InputPayload::Prompt { prompt_id, .. } => prompt_id,
        };
        if payload_prompt_id.is_empty() {
            return Err("input prompt identity is empty".into());
        }
        let requested_route = match &payload {
            crate::session::input_inbox::InputPayload::Prompt {
                requested_route, ..
            } => requested_route,
        };
        let route_matches_intent = matches!(
            (intent, requested_route),
            (
                chat_state::InputIntent::Prompt | chat_state::InputIntent::Followup,
                crate::session::input_inbox::InputRequestRoute::Fifo
            ) | (
                chat_state::InputIntent::Steer,
                crate::session::input_inbox::InputRequestRoute::Steer { .. }
            )
        );
        if !route_matches_intent {
            return Err("input route does not match its immutable request intent".into());
        }
        if prompt_id
            .as_deref()
            .is_some_and(|id| id != payload_prompt_id)
        {
            return Err("input prompt identity does not match its immutable payload".into());
        }
        let prompt_id = payload_prompt_id.clone();
        let input_id = crate::session::input_inbox::input_id_for_prompt(
            self.session_info.id.0.as_ref(),
            &prompt_id,
        );
        // Canonical pre-check: fold the durable Timeline through the stable
        // input identity, then read only the hit payload_ref to verify the
        // original immutable request. Never scan every historical blob to
        // discover the prompt's input.
        match self.chat_state_handle.input_state(&input_id).await {
            Err(error) => {
                return Err(format!(
                    "Timeline unavailable while reconciling input identity: {error}"
                ));
            }
            Ok(Some(lifecycle)) => {
                let state = self
                    .verify_existing_input(lifecycle, intent, &payload)
                    .await?;
                return Ok(InputAdmissionAttempt::Existing { input_id, state });
            }
            Ok(None) => {}
        }

        let crate::session::input_inbox::InputPayload::Prompt {
            prompt_blocks,
            skill_invocations,
            ..
        } = &mut payload;
        *skill_invocations = self.freeze_skill_invocations(prompt_blocks).await;
        let artifact_guard = self.input_artifact_gate.lock().await;
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
        let (payload_ref, hook_prompt) = tokio::task::spawn_blocking(move || {
            let reference = crate::session::input_inbox::write_payload(&directory, &payload)?;
            // Serialization/decoded-image budgets have now been checked. The
            // Hook reviews this exact snapshot, never a mutable source path.
            Ok::<_, std::io::Error>((reference, payload.hook_prompt()))
        })
        .await
        .map_err(|error| format!("input artifact writer failed: {error}"))?
        .map_err(|error| format!("input artifact write failed: {error}"))?;

        let submitted = self
            .chat_state_handle
            .submit_input_durably(input_id.clone(), intent, payload_ref.clone())
            .await
            .map_err(|error| format!("input submission was not durable: {error}"))?;
        if !submitted.created {
            drop(artifact_guard);
            let state = match self.chat_state_handle.input_state(&input_id).await {
                Ok(Some(lifecycle)) => lifecycle.state,
                Ok(None) => {
                    return Err(format!(
                        "input identity evidence is unavailable for prompt {prompt_id} (input {input_id})"
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "Timeline unavailable while reconciling input identity: {error}"
                    ));
                }
            };
            return Ok(InputAdmissionAttempt::Existing { input_id, state });
        }
        // The blob is now reachable from Timeline, so maintenance can no longer
        // classify it as an orphan. Do not serialize slow external Hook runs on
        // the artifact-maintenance gate.
        drop(artifact_guard);

        let envelope = self.make_hook_envelope(
            ::hooks::event::HookEventName::UserPromptSubmit,
            Some(prompt_id),
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
        Ok(InputAdmissionAttempt::New { input_id, decision })
    }

    /// Verify that a canonical lifecycle hit carries the exact same immutable
    /// request, then return its current state as a replay observation.
    /// Server-side Prompt/Followup derivation for a FIFO payload does not
    /// conflict; an explicit steer intent or any payload/route difference does.
    async fn verify_existing_input(
        &self,
        lifecycle: chat_state::InputLifecycle,
        intent: chat_state::InputIntent,
        requested: &crate::session::input_inbox::InputPayload,
    ) -> Result<chat_state::InputState, String> {
        let input_id = lifecycle.input_id.clone();
        let prompt_id = match requested {
            crate::session::input_inbox::InputPayload::Prompt { prompt_id, .. } => prompt_id,
        };
        // Prompt/Followup is a server-side FIFO derivation; the payload
        // comparison below still pins the immutable requested_route.
        let intent_compatible = lifecycle.intent == intent
            || matches!(
                (lifecycle.intent, intent),
                (
                    chat_state::InputIntent::Prompt | chat_state::InputIntent::Followup,
                    chat_state::InputIntent::Prompt | chat_state::InputIntent::Followup,
                )
            );
        if !intent_compatible {
            return Err(format!(
                "input identity conflict for prompt {prompt_id}: intent differs"
            ));
        }
        let existing_payload = crate::session::input_inbox::read_payload(
            &self.session_directory,
            &lifecycle.payload_ref,
        )
        .map_err(|error| {
            format!("input payload evidence is unavailable for {input_id}: {error}")
        })?;
        let existing_prompt_id = match &existing_payload {
            crate::session::input_inbox::InputPayload::Prompt { prompt_id, .. } => prompt_id,
        };
        if existing_prompt_id != prompt_id {
            return Err(format!(
                "input identity evidence mismatch for prompt {prompt_id} (input {input_id})"
            ));
        }
        let existing_value = comparable_input_payload(&existing_payload)?;
        let requested_value = comparable_input_payload(requested)?;
        if existing_value != requested_value {
            return Err(format!(
                "input identity conflict for prompt {prompt_id}: immutable payload or route differs"
            ));
        }
        Ok(lifecycle.state)
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
        Ok(AdmittedInput {
            input_id,
            replayed: false,
            replay_state: None,
        })
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
        display_echo: Option<chat_state::UserDisplayEcho>,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
            display_echo,
            skill_loads: Vec::new(),
        })
        .await
    }

    pub(super) async fn consume_fifo_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
        item: sampling_types::ConversationItem,
        display_echo: Option<chat_state::UserDisplayEcho>,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
            display_echo,
            skill_loads: Vec::new(),
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
        // One canonical runtime query for the whole reconstruction: a Steer
        // whose target Turn is still preserved by a suspended/restoring
        // checkpoint must return to the actor's interjection buffer with its
        // original identity instead of being rerouted into the FIFO (which the
        // Timeline validator rejects while the Turn stays open). Any other
        // runtime phase keeps the existing deterministic reroute path.
        let preserved_turn = match self.chat_state_handle.runtime_state().await {
            Ok(lifecycle) if lifecycle.preserves_open_turn() => lifecycle
                .checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.turn.as_ref().map(|turn| turn.turn_id)),
            Ok(_) => None,
            Err(error) => {
                return Err(format!(
                    "runtime state unavailable while restoring pending inputs: {error}"
                ));
            }
        };
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
            .map_err(|error| format!("input artifact reader failed: {error}"))?;
            let payload = match payload {
                Ok(payload) => payload,
                Err(error) if crate::session::input_inbox::is_attachment_error(&error) => {
                    // Keep Submitted and its references as audit evidence. An
                    // allowed input whose snapshot cannot be restored must not
                    // become a text-only turn or prevent unrelated history use.
                    self.dismiss_input_ids(
                        [pending.input_id.clone()],
                        chat_state::InputDismissReason::Invalidated,
                    )
                    .await?;
                    self.persist_ui_notice(crate::extensions::notification::UiNotice {
                        correlation_id: pending.input_id,
                        category: crate::extensions::notification::UiNoticeCategory::Lifecycle,
                        subject: Some("input attachment".into()),
                        description: None,
                        message: "A pending input was stopped because an image attachment is unavailable. Reattach the image and submit a new prompt.".into(),
                        tone: crate::extensions::notification::UiNoticeTone::Error,
                        details: Some(error.to_string()),
                    }).await?;
                    continue;
                }
                Err(error) => return Err(format!("input payload is missing or corrupt: {error}")),
            };
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
                        requested_route: _,
                        prompt_blocks,
                        skill_invocations: _,
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
                    let steer_target = match pending.route {
                        chat_state::InputRoute::Steer { target_turn } => Some(target_turn),
                        chat_state::InputRoute::Fifo => None,
                    };
                    if let Some(target_turn) = steer_target {
                        if preserved_turn == Some(target_turn) {
                            // The checkpoint preserved the target Turn, so the
                            // input's canonical route is still Steer(target).
                            // Rebuild the actor's interjection buffer entry
                            // with the original identity and full content; no
                            // FIFO row, no reroute, no repeated Hook/Submitted.
                            // The runtime release barrier still decides when
                            // the resumed Turn may drain and consume it.
                            let text = Self::queue_text_from_blocks(&prompt_blocks);
                            let attachments = prompt_blocks
                                .into_iter()
                                .filter_map(|block| match block {
                                    acp::ContentBlock::Image(image) => Some(image),
                                    _ => None,
                                })
                                .collect::<Vec<_>>();
                            self.pending_interjections.push(
                                super::interjection::PendingInterjection {
                                    text,
                                    attachments,
                                    requeue: Some(
                                        super::interjection::ResidualInterjectionRequeue {
                                            input_ids: vec![pending.input_id],
                                            prompt_id,
                                            origin,
                                            turn_kind,
                                            client_identifier,
                                            screen_mode,
                                            verbatim,
                                            json_schema,
                                        },
                                    ),
                                },
                            );
                            continue;
                        }
                        self.reroute_input(pending.input_id.clone(), chat_state::InputRoute::Fifo)
                            .await?;
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
                    // The durable queue row owns the runtime/display identity.
                    // An edited row's immutable payload keeps its own
                    // `original:edit:N` admission identity; reusing it as the
                    // runtime prompt id would split the restored row from its
                    // queue identity.
                    let prompt_id = queue_meta.id.clone();
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
            match tokio::task::spawn_blocking(move || {
                crate::session::input_inbox::remove_payload_hashes(&directory, &hashes)
            })
            .await
            {
                Ok(Ok(_)) => {}
                error => {
                    tracing::warn!(?error, "input manifest reconciliation failed");
                    break;
                }
            }
        }
        drop(batch_rx);
        let _ = producer.await;
        if shutdown.is_cancelled() {
            return;
        }
        // Includes crashes after image writes but before the manifest exists.
        // Hold the same gate across the root snapshot and the attachment sweep.
        let _guard = self.input_artifact_gate.lock().await;
        let Some(retained) = self
            .chat_state_handle
            .submitted_input_payload_hashes()
            .await
        else {
            return;
        };
        let Ok(directory) = self.session_directory.try_clone() else {
            return;
        };
        let shutdown = shutdown.clone();
        match tokio::task::spawn_blocking(move || {
            crate::session::input_inbox::reconcile_image_blobs(&directory, &retained, || {
                shutdown.is_cancelled()
            })
        })
        .await
        {
            Ok(Ok(_)) => {}
            error => tracing::warn!(?error, "input image reconciliation failed"),
        }
    }

    pub(super) async fn reconcile_user_image_assets(
        &self,
        shutdown: &tokio_util::sync::CancellationToken,
    ) {
        if shutdown.is_cancelled() {
            return;
        }
        let _guard = tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            guard = self.input_artifact_gate.lock() => guard,
        };
        if shutdown.is_cancelled() {
            return;
        }
        // The live owner has already validated every committed physical User
        // reference, including those no longer in the current Surface. Keep
        // publication excluded from this query through the completed sweep.
        let retained_paths = tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            paths = self.chat_state_handle.committed_user_image_paths() => match paths {
                Ok(paths) => paths,
                Err(error) => {
                    tracing::warn!(%error, "user image asset authority unavailable");
                    return;
                }
            },
        };
        if shutdown.is_cancelled() {
            return;
        }
        let Ok(directory) = self.session_directory.try_clone() else {
            return;
        };
        let shutdown = shutdown.clone();
        // Once admitted, deletion stays owned until its blocking task actually
        // returns. Cancellation stops between batches; it never detaches I/O.
        match tokio::task::spawn_blocking(move || {
            crate::session::image_describe::reconcile_user_image_assets(
                &directory,
                &retained_paths,
                || shutdown.is_cancelled(),
            )
        })
        .await
        {
            Ok(Ok(_)) => {}
            error => tracing::warn!(?error, "user image asset reconciliation failed"),
        }
    }
}

pub(super) fn input_block_reason_message(reason: chat_state::InputBlockReason) -> String {
    match reason {
        chat_state::InputBlockReason::Hook { reason } => reason,
        chat_state::InputBlockReason::StaleSteerTarget => {
            "the target turn is no longer running".into()
        }
        chat_state::InputBlockReason::ProcessInterrupted => {
            "input admission was interrupted".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::input_inbox::{self, InputPayload};

    fn explicit_selection_payload(prompt_id: &str) -> InputPayload {
        let mut payload =
            input_inbox::tests::text_payload(prompt_id, "/__grow_test_missing_skill__ args");
        let InputPayload::Prompt { prompt_blocks, .. } = &mut payload;
        let mut text = acp::TextContent::new("/__grow_test_missing_skill__ args");
        text.meta = Some(
            [(
                "grow/skillSelection".into(),
                serde_json::json!({
                    "name": "__grow_test_missing_skill__",
                    "path": "/__grow_test/missing/SKILL.md",
                    "plugin_name": null
                }),
            )]
            .into_iter()
            .collect(),
        );
        *prompt_blocks = vec![acp::ContentBlock::Text(text)];
        payload
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_image_asset_reconciliation_retains_current_and_historical_owner_refs() {
        use crate::session::image_describe::{persist_user_images, render_image_files_block};
        use std::future::Future as _;

        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let historical = persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("AQID", "image/png")],
                )
                .unwrap();
                let historical_message =
                    render_image_files_block(&[historical[0].to_string_lossy().into_owned()])
                        .unwrap();
                actor
                    .chat_state_handle
                    .push_user_message_durably(sampling_types::ConversationItem::user(
                        historical_message.clone(),
                    ))
                    .await
                    .unwrap();
                let (mut replacement, revision) = actor
                    .chat_state_handle
                    .get_conversation_with_revision()
                    .await
                    .unwrap();
                // ContextRebuild must preserve the canonical stable System head.
                replacement
                    .retain(|item| matches!(item, sampling_types::ConversationItem::System(_)));
                replacement.push(sampling_types::ConversationItem::user_meta(
                    "replacement context",
                ));
                actor
                    .chat_state_handle
                    .replace_context_durably(replacement, revision)
                    .await
                    .unwrap();
                assert!(
                    !format!("{:?}", actor.chat_state_handle.get_conversation().await)
                        .contains(&historical[0].to_string_lossy().into_owned())
                );

                // Publication owns the same gate. A reconciliation started here
                // must query only after the new User reference is committed.
                let publication = actor.input_artifact_gate.lock().await;
                let current = persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("BAUG", "image/png")],
                )
                .unwrap();
                let orphan = persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("BwgJ", "image/png")],
                )
                .unwrap();
                let staging = actor
                    .session_dir
                    .join("assets")
                    .join(format!(".images-{}.tmp", uuid::Uuid::new_v4()));
                std::fs::create_dir(&staging).unwrap();
                let unknown = actor.session_dir.join("assets/user-owned");
                std::fs::create_dir(&unknown).unwrap();
                // The owner is valid; this disk source must never be replayed
                // by live maintenance, even when deletable assets exist.
                let disk_evidence = b"{broken timeline}\n";
                std::fs::write(actor.session_dir.join("timeline.jsonl"), disk_evidence).unwrap();
                let shutdown = tokio_util::sync::CancellationToken::new();
                let cleanup = actor.reconcile_user_image_assets(&shutdown);
                tokio::pin!(cleanup);
                std::future::poll_fn(|cx| {
                    assert!(cleanup.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                actor
                    .chat_state_handle
                    .push_user_message_durably(sampling_types::ConversationItem::user(
                        render_image_files_block(&[current[0].to_string_lossy().into_owned()])
                            .unwrap(),
                    ))
                    .await
                    .unwrap();
                drop(publication);
                cleanup.await;
                assert!(historical[0].is_file());
                assert!(current[0].is_file());
                assert!(!orphan[0].parent().unwrap().exists());
                assert!(!staging.exists());
                assert!(unknown.is_dir());
                assert_eq!(
                    std::fs::read(actor.session_dir.join("timeline.jsonl")).unwrap(),
                    disk_evidence
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_image_asset_reconciliation_preserves_assets_when_owner_exits() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let owner_shutdown = tokio_util::sync::CancellationToken::new();
                let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn(
                        vec![],
                        config,
                        Box::new(chat_state::NullTimelinePersistence),
                        events,
                        owner_shutdown.clone(),
                    );
                // Establish the real owner before terminating it; this is not
                // an injected empty result or a no-op handle.
                assert!(
                    actor
                        .chat_state_handle
                        .committed_user_image_paths()
                        .await
                        .unwrap()
                        .is_empty()
                );
                owner_shutdown.cancel();
                assert!(
                    actor
                        .chat_state_handle
                        .committed_user_image_paths()
                        .await
                        .is_err()
                );
                let orphan = crate::session::image_describe::persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("AQID", "image/png")],
                )
                .unwrap();
                let staging = actor
                    .session_dir
                    .join("assets")
                    .join(format!(".images-{}.tmp", uuid::Uuid::new_v4()));
                std::fs::create_dir(&staging).unwrap();
                actor
                    .reconcile_user_image_assets(&tokio_util::sync::CancellationToken::new())
                    .await;
                assert!(orphan[0].is_file());
                assert!(staging.is_dir());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_image_asset_reconciliation_cancellation_stops_before_authority_query() {
        use std::future::Future as _;

        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let orphan = crate::session::image_describe::persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("AQID", "image/png")],
                )
                .unwrap();
                let shutdown = tokio_util::sync::CancellationToken::new();
                let publication = actor.input_artifact_gate.lock().await;
                let cleanup = actor.reconcile_user_image_assets(&shutdown);
                tokio::pin!(cleanup);
                std::future::poll_fn(|cx| {
                    assert!(cleanup.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                shutdown.cancel();
                tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
                    .await
                    .unwrap();
                assert!(orphan[0].is_file());
                drop(publication);
                actor.reconcile_user_image_assets(&shutdown).await;
                assert!(orphan[0].is_file());
                assert!(
                    actor
                        .chat_state_handle
                        .committed_user_image_paths()
                        .await
                        .is_ok()
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_image_asset_reconciliation_cancellation_keeps_pending_owner_write() {
        use std::future::Future as _;

        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let (persistence, mut acknowledgements) =
                    chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
                let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn(
                        vec![],
                        config,
                        Box::new(persistence),
                        events,
                        tokio_util::sync::CancellationToken::new(),
                    );
                let orphan = crate::session::image_describe::persist_user_images(
                    &actor.session_directory,
                    &[acp::ImageContent::new("AQID", "image/png")],
                )
                .unwrap();
                let write = actor.chat_state_handle.push_user_message_durably(
                    sampling_types::ConversationItem::user("pending durable message"),
                );
                tokio::pin!(write);
                std::future::poll_fn(|cx| {
                    assert!(write.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                let ack = acknowledgements.next_timeline_ack().await.unwrap();
                let shutdown = tokio_util::sync::CancellationToken::new();
                let cleanup = actor.reconcile_user_image_assets(&shutdown);
                tokio::pin!(cleanup);
                std::future::poll_fn(|cx| {
                    assert!(cleanup.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                shutdown.cancel();
                tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
                    .await
                    .unwrap();
                assert!(orphan[0].is_file());
                // Stopping optional maintenance does not cancel the canonical
                // owner, its accepted write, or the original acknowledgement.
                ack.send(Ok(())).unwrap();
                write.await.unwrap();
                assert!(
                    actor
                        .chat_state_handle
                        .committed_user_image_paths()
                        .await
                        .is_ok()
                );
                assert!(orphan[0].is_file());
                assert!(actor.input_artifact_gate.try_lock().is_ok());
            })
            .await;
    }

    #[test]
    fn replay_payload_comparison_ignores_only_frozen_host_skill_refs() {
        let original = input_inbox::tests::text_payload("stable-prompt", "/local:commit fix");
        let mut frozen = original.clone();
        let InputPayload::Prompt {
            skill_invocations, ..
        } = &mut frozen;
        *skill_invocations = vec![crate::session::input_inbox::FrozenSkillInvocation {
            callable_name: "commit".into(),
            qualified_name: "local:commit".into(),
            args: "fix".into(),
            skill: Some(tools::implementations::skills::types::SkillInfo {
                name: "commit".into(),
                path: "/skills/commit/SKILL.md".into(),
                body: Some("frozen body".into()),
                ..Default::default()
            }),
            selection_error: None,
        }];

        assert_eq!(
            comparable_input_payload(&original).unwrap(),
            comparable_input_payload(&frozen).unwrap(),
        );
        let InputPayload::Prompt { prompt_blocks, .. } = &mut frozen;
        *prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new("changed"))];
        assert_ne!(
            comparable_input_payload(&original).unwrap(),
            comparable_input_payload(&frozen).unwrap(),
            "original user content remains part of replay identity"
        );
    }

    #[test]
    fn explicit_skill_selection_metadata_is_strict_and_host_readable() {
        let mut text = acp::TextContent::new("/acme:deploy prod");
        text.meta = Some(
            [(
                "grow/skillSelection".into(),
                serde_json::json!({
                    "name": "acme:deploy",
                    "path": "/plugins/acme/deploy/SKILL.md",
                    "plugin_name": "acme"
                }),
            )]
            .into_iter()
            .collect(),
        );
        let selected = SessionActor::explicit_skill_selection(&[acp::ContentBlock::Text(text)])
            .unwrap()
            .expect("metadata carries explicit selection identity");
        assert_eq!(selected.name, "acme:deploy");
        assert_eq!(selected.path, "/plugins/acme/deploy/SKILL.md");
        assert_eq!(selected.plugin_name.as_deref(), Some("acme"));

        let mut malformed = acp::TextContent::new("/acme:deploy prod");
        malformed.meta = Some(
            [(
                "grow/skillSelection".into(),
                serde_json::json!({"name": "deploy"}),
            )]
            .into_iter()
            .collect(),
        );
        assert!(
            SessionActor::explicit_skill_selection(&[acp::ContentBlock::Text(malformed)]).is_err()
        );
    }

    #[test]
    fn explicit_skill_selection_requires_exact_resolved_source() {
        let selected = ExplicitSkillSelection {
            name: "acme:deploy".into(),
            path: "/plugins/acme/deploy/SKILL.md".into(),
            plugin_name: Some("acme".into()),
        };
        let resolved = crate::session::slash_commands::ParsedSkillRef {
            name: "acme:deploy".into(),
            args: "prod".into(),
            skill_path: "/plugins/acme/deploy/SKILL.md".into(),
            qualified_name: "acme:deploy".into(),
            plugin_name: Some("acme".into()),
        };
        assert!(explicit_selection_matches(&selected, &resolved));
        let mut stale = resolved.clone();
        stale.skill_path = "/plugins/acme-v2/deploy/SKILL.md".into();
        assert!(!explicit_selection_matches(&selected, &stale));
        stale = resolved;
        stale.plugin_name = Some("other".into());
        assert!(!explicit_selection_matches(&selected, &stale));
    }

    struct JsonlTimelinePersistence {
        storage: crate::session::storage::jsonl::JsonlStorageAdapter,
        info: crate::session::info::Info,
    }

    impl chat_state::TimelinePersistence for JsonlTimelinePersistence {
        fn persist_timeline_event_and_ack(
            &mut self,
            event: &chat_state::TimelineEvent,
        ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
            use crate::session::storage::StorageAdapter as _;

            let storage = self.storage.clone();
            let info = self.info.clone();
            let event = event.clone();
            let (respond_to, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let _ = respond_to.send(storage.append_timeline_event_durable(&info, &event).await);
            });
            receiver
        }

        fn flush(&mut self) {}
    }

    async fn admit_image(actor: &SessionActor) -> String {
        actor
            .admit_human_input(
                chat_state::InputIntent::Prompt,
                input_inbox::tests::image_payload(),
                Some("prompt-1".into()),
                chat_state::InputRoute::Fifo,
                vec![],
            )
            .await
            .unwrap()
            .input_id
    }

    #[tokio::test(flavor = "current_thread")]
    async fn physical_writer_replacement_replays_stable_input_identity_once() {
        const ROOT_ENV: &str = "GROW_TEST_STABLE_INPUT_REPLACEMENT_ROOT";
        const READY_ENV: &str = "GROW_TEST_STABLE_INPUT_REPLACEMENT_READY";
        const TEST: &str = "session::actor::input_admission::tests::physical_writer_replacement_replays_stable_input_identity_once";
        const PROMPT_ID: &str = "stable-writer-replacement-prompt";
        const READY_BYTES: &[u8] = b"submitted and Hook facts durable";

        fn fixture_info() -> crate::session::info::Info {
            crate::session::info::Info {
                id: acp::SessionId::new("test-actor"),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            }
        }

        if let Ok(root) = std::env::var(ROOT_ENV) {
            use crate::session::storage::StorageAdapter as _;
            use chat_state::{HookEvent, HookEventType, Timeline, TimelineEventKind};

            let root = std::path::PathBuf::from(root);
            let ready = std::path::PathBuf::from(std::env::var(READY_ENV).unwrap());
            let info = fixture_info();
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root);
            storage
                .init_session(&info, crate::session::persistence::default_model_id())
                .await
                .unwrap();
            let opened = storage.open_session(&info).unwrap();
            let payload = input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input");
            let payload_ref = input_inbox::write_payload(opened.directory(), &payload).unwrap();
            let input_id = input_inbox::input_id_for_prompt(info.id.0.as_ref(), PROMPT_ID);
            let occurrence_id = format!("hook-{input_id}");
            let mut timeline = Timeline::default();
            let submitted = timeline
                .record(TimelineEventKind::Input(
                    chat_state::InputEvent::Submitted {
                        input_id: input_id.clone(),
                        intent: chat_state::InputIntent::Prompt,
                        payload_ref,
                    },
                ))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &submitted)
                .await
                .unwrap();
            let triggered = timeline
                .record(TimelineEventKind::Hook(HookEvent::Triggered {
                    occurrence_id: occurrence_id.clone(),
                    event: HookEventType::UserPromptSubmit,
                    gate: chat_state::HookGateKind::Prompt,
                    cause: chat_state::HookCause::Input {
                        input_id: input_id.clone(),
                    },
                    config_generation: 1,
                    handlers: Vec::new(),
                }))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &triggered)
                .await
                .unwrap();
            let completed = timeline
                .record(TimelineEventKind::Hook(HookEvent::Completed {
                    occurrence_id,
                    decision: chat_state::HookAggregateDecision::Prompt {
                        decision: chat_state::HookGateDecision::Allow,
                    },
                }))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &completed)
                .await
                .unwrap();
            std::fs::write(ready, READY_BYTES).expect("signal durable fixture readiness");
            loop {
                std::thread::park_timeout(std::time::Duration::from_secs(60));
            }
        }

        struct KillChildOnDrop(std::process::Child);
        impl Drop for KillChildOnDrop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let temp = tempfile::TempDir::new().expect("isolated writer root");
        let root = temp.path().join("session-storage");
        let ready = temp.path().join("child-ready");
        #[expect(
            clippy::disallowed_methods,
            reason = "This subprocess fixture owns a kill/wait guard or bounded poll/wait loop; it is independent of session processes"
        )]
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(ROOT_ENV, &root)
            .env(READY_ENV, &ready)
            .spawn()
            .unwrap();
        let mut child = KillChildOnDrop(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline
            && !std::fs::read(&ready).is_ok_and(|bytes| bytes == READY_BYTES)
        {
            if let Some(status) = child.0.try_wait().unwrap() {
                panic!("fixture writer exited before readiness: {status}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read(&ready).unwrap(),
            READY_BYTES,
            "child must finish publishing its readiness marker after durable input facts"
        );

        let info = fixture_info();
        let observer = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.clone());
        assert!(
            observer.claim_recovery_writer(&info).is_err(),
            "the physical writer process must exclude its replacement"
        );
        child.0.kill().unwrap();
        let _ = child.0.wait().unwrap();

        let replacement = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root);
        replacement
            .claim_recovery_writer(&info)
            .expect("process exit releases the OS writer lease");
        let loaded = replacement
            .load_session_for_write_without_updates(&info)
            .await
            .expect("replacement loads the committed session");
        let original_input_id = input_inbox::input_id_for_prompt(info.id.0.as_ref(), PROMPT_ID);
        let original_lifecycle = loaded
            .timeline
            .input_state(&original_input_id)
            .expect("replacement recovers Submitted state before lifecycle repair");
        assert!(matches!(
            &original_lifecycle.state,
            chat_state::InputState::Submitted
        ));
        let original_payload_ref = loaded
            .timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                    input_id,
                    payload_ref,
                    ..
                }) if input_id == &original_input_id => Some(payload_ref.clone()),
                _ => None,
            })
            .expect("durable Submitted payload reference");
        let original_payload = input_inbox::read_payload(
            replacement.open_session(&info).unwrap().directory(),
            &original_payload_ref,
        )
        .expect("replacement can read the complete original payload");
        assert_eq!(
            serde_json::to_value(&original_payload).unwrap(),
            serde_json::to_value(input_inbox::tests::text_payload(
                PROMPT_ID,
                "durable submitted input"
            ))
            .unwrap()
        );

        let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let chat_state = chat_state::ChatStateActor::spawn_from_timeline(
            loaded.timeline.events().to_vec(),
            sampling_types::SamplingConfig {
                base_url: "http://localhost".into(),
                model: "test".into(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                query_params: Default::default(),
                env_http_headers: Default::default(),
                context_window: std::num::NonZeroU64::new(256_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            Box::new(JsonlTimelinePersistence {
                storage: replacement.clone(),
                info: info.clone(),
            }),
            chat_event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("replacement ChatState actor");
        let replaced_events = chat_state.timeline_events().await.unwrap();
        assert!(matches!(
            replaced_events.iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Input(
                    chat_state::InputEvent::AdmissionResolved {
                        input_id,
                        decision:
                            chat_state::InputAdmissionDecision::Block {
                                reason: chat_state::InputBlockReason::ProcessInterrupted,
                            },
                        ..
                    },
                ) if input_id == &original_input_id => Some(()),
                _ => None,
            }),
            Some(())
        ));

        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let directory = replacement.open_session(&info).unwrap().directory_handle();
                let directory_path = directory.display_path().to_path_buf();
                let actor = std::sync::Arc::get_mut(&mut actor)
                    .expect("test actor has one owner before injection");
                actor.session_dir = directory_path;
                actor.session_directory = directory;
                actor.events = crate::session::events::EventTracker::new(chat_state.clone());
                actor.chat_state_handle = chat_state.clone();

                let before_retry = chat_state.timeline_events().await.unwrap();
                let attempt = actor
                    .run_human_input_admission_hook(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input"),
                        Some(PROMPT_ID.into()),
                    )
                    .await
                    .expect("stable retry reconciles against reloaded identity");
                assert!(matches!(
                    attempt,
                    InputAdmissionAttempt::Existing {
                        input_id,
                        state: chat_state::InputState::Blocked {
                            reason: chat_state::InputBlockReason::ProcessInterrupted,
                        },
                    } if input_id == original_input_id
                ));
                let replayed = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input"),
                        Some(PROMPT_ID.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("interrupted stable input replays its canonical observation");
                assert_eq!(replayed.input_id, original_input_id);
                assert!(replayed.replayed);
                assert!(matches!(
                    replayed.replay_state,
                    Some(chat_state::InputState::Blocked {
                        reason: chat_state::InputBlockReason::ProcessInterrupted,
                    })
                ));
                let after_retry = chat_state.timeline_events().await.unwrap();
                assert_eq!(after_retry.len(), before_retry.len());
                assert_eq!(
                    after_retry
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id, .. }
                            ) if input_id == &original_input_id
                        ))
                        .count(),
                    1,
                    "replacement retry keeps one durable Submitted fact"
                );
                assert_eq!(
                    after_retry
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id },
                                    ..
                                }
                            ) if input_id == &original_input_id
                        ))
                        .count(),
                    1,
                    "replacement retry does not run the Hook again"
                );
                assert!(!after_retry.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                        | chat_state::TimelineEventKind::Request(_)
                        | chat_state::TimelineEventKind::Tool(_)
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_identity_replay_reuses_admission_and_rejects_payload_conflicts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "same request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("first request admitted");
                assert!(!first.replayed);
                let after_first = actor.chat_state_handle.timeline_events().await.unwrap();
                let replay = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "same request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("exact retry reuses admission");
                assert!(replay.replayed);
                assert_eq!(replay.input_id, first.input_id);
                let after_replay = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(after_replay.len(), after_first.len());

                let submitted_payload = after_first
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id,
                                payload_ref,
                                ..
                            },
                        ) if input_id == &first.input_id => Some(
                            input_inbox::read_payload(&actor.session_directory, payload_ref)
                                .unwrap(),
                        ),
                        _ => None,
                    })
                    .expect("submitted payload snapshot");
                assert_eq!(
                    serde_json::to_value(submitted_payload).unwrap(),
                    serde_json::to_value(input_inbox::tests::text_payload(
                        "stable-prompt",
                        "same request"
                    ))
                    .unwrap(),
                    "replay must retain the original immutable payload snapshot"
                );

                let conflict = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "changed request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await;
                assert!(conflict.is_err());
                assert_eq!(
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    after_first.len(),
                    "a conflicting payload must fail before Hook or admission"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_skill_input_replay_reuses_frozen_selection_without_duplicate_admission() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let prompt_id = "explicit-skill-replay";
                let original = explicit_selection_payload(prompt_id);
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        original.clone(),
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("explicit selection snapshot is admitted");
                let events_after_first = actor.chat_state_handle.timeline_events().await.unwrap();
                let replay = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        original,
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("exact retry reuses its frozen selection");
                assert!(replay.replayed);
                assert_eq!(replay.input_id, first.input_id);
                let events_after_replay = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(events_after_replay.len(), events_after_first.len());
                assert_eq!(
                    events_after_replay
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id, .. }
                            ) if input_id == &first.input_id
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    events_after_replay
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id },
                                    ..
                                }
                            ) if input_id == &first.input_id
                        ))
                        .count(),
                    1
                );

                let frozen = events_after_replay
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id,
                                payload_ref,
                                ..
                            },
                        ) if input_id == &first.input_id => Some(
                            input_inbox::read_payload(&actor.session_directory, payload_ref)
                                .expect("frozen payload body remains readable"),
                        ),
                        _ => None,
                    })
                    .expect("one immutable Submitted payload");
                let InputPayload::Prompt {
                    prompt_blocks,
                    skill_invocations,
                    ..
                } = frozen;
                assert_eq!(
                    prompt_blocks,
                    match explicit_selection_payload(prompt_id) {
                        InputPayload::Prompt { prompt_blocks, .. } => prompt_blocks,
                    }
                );
                assert_eq!(skill_invocations.len(), 1);
                assert_eq!(
                    skill_invocations[0].qualified_name,
                    "__grow_test_missing_skill__"
                );
                assert!(skill_invocations[0].skill.is_none());
                assert!(skill_invocations[0].selection_error.is_some());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_input_admission_recovers_blocked_without_repeating_hook_or_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let prompt_id = "interrupted-admission-prompt";
                let payload = input_inbox::tests::text_payload(prompt_id, "preserve this payload");
                let attempt = actor
                    .run_human_input_admission_hook(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                    )
                    .await
                    .expect("submission and Hook facts are durable");
                let input_id = match attempt {
                    InputAdmissionAttempt::New {
                        input_id,
                        decision: chat_state::InputAdmissionDecision::Allow,
                    } => input_id,
                    _ => panic!("fixture Hook should leave a new Allow decision unresolved"),
                };

                let after_hook = actor.chat_state_handle.timeline_events().await.unwrap();
                let submitted_payload = after_hook
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id: submitted_id,
                                payload_ref,
                                ..
                            },
                        ) if submitted_id == &input_id => Some(
                            input_inbox::read_payload(&actor.session_directory, payload_ref)
                                .expect("original payload remains readable"),
                        ),
                        _ => None,
                    })
                    .expect("Submitted evidence exists");
                assert_eq!(
                    serde_json::to_value(&submitted_payload).unwrap(),
                    serde_json::to_value(&payload).unwrap()
                );
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id: id, .. }
                            ) if id == &input_id
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id: id },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1
                );
                let hook_occurrence_id = after_hook
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            occurrence_id,
                            event: chat_state::HookEventType::UserPromptSubmit,
                            cause: chat_state::HookCause::Input { input_id: id },
                            ..
                        }) if id == &input_id => Some(occurrence_id),
                        _ => None,
                    })
                    .expect("input Hook occurrence");
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Completed { occurrence_id, .. }
                            ) if occurrence_id == hook_occurrence_id
                        ))
                        .count(),
                    1,
                    "fixture stops after the Hook has durably completed"
                );
                assert!(!after_hook.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::AdmissionResolved { input_id: id, .. }
                    ) if id == &input_id
                )));

                let unresolved_retry = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("unresolved retry observes the durable Submitted state");
                assert!(unresolved_retry.replayed);
                assert_eq!(
                    unresolved_retry.replay_state,
                    Some(chat_state::InputState::Submitted)
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    after_hook.len(),
                    "an unresolved retry must not repeat Submitted or Hook facts"
                );

                let recovery = actor
                    .chat_state_handle
                    .recover_interrupted_durably()
                    .await
                    .expect("real durable interrupted-work recovery");
                assert!(recovery.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::AdmissionResolved {
                            input_id: id,
                            decision: chat_state::InputAdmissionDecision::Block {
                                reason: chat_state::InputBlockReason::ProcessInterrupted
                            },
                            ..
                        }
                    ) if id == &input_id
                )));

                let recovered_retry = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("blocked retry observes the durable interruption");
                assert!(recovered_retry.replayed);
                assert!(matches!(
                    recovered_retry.replay_state,
                    Some(chat_state::InputState::Blocked {
                        reason: chat_state::InputBlockReason::ProcessInterrupted,
                    })
                ));
                actor.restore_pending_human_inputs().await.unwrap();
                assert!(
                    actor.state.lock().await.pending_inputs.is_empty(),
                    "ProcessInterrupted inputs must not be restored into the FIFO"
                );

                let final_events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id: id, .. }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "recovery and retry preserve one immutable submission"
                );
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id: id },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "retry must not run a second Hook"
                );
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::AdmissionResolved {
                                    input_id: id,
                                    decision: chat_state::InputAdmissionDecision::Block {
                                        reason: chat_state::InputBlockReason::ProcessInterrupted
                                    },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "interruption recovery resolves admission exactly once"
                );
                assert!(!final_events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                        | chat_state::TimelineEventKind::Request(_)
                        | chat_state::TimelineEventKind::Tool(_)
                )));
                let recovered_payload_ref = final_events
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id: id,
                                payload_ref,
                                ..
                            },
                        ) if id == &input_id => Some(payload_ref),
                        _ => None,
                    })
                    .expect("original submission still anchors its payload");
                assert_eq!(
                    serde_json::to_value(
                        input_inbox::read_payload(&actor.session_directory, recovered_payload_ref)
                            .unwrap()
                    )
                    .unwrap(),
                    serde_json::to_value(payload).unwrap(),
                    "interruption recovery must retain the original payload artifact"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_identity_rejects_fifo_steer_and_steer_target_conflicts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let mut fifo = input_inbox::tests::text_payload("route-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut fifo;
                *requested_route = input_inbox::InputRequestRoute::Fifo;
                actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        fifo,
                        Some("route-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("initial FIFO input admitted");

                let mut steer = input_inbox::tests::text_payload("route-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut steer;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "1".into(),
                };
                assert!(
                    actor
                        .admit_human_input(
                            chat_state::InputIntent::Steer,
                            steer,
                            Some("route-prompt".into()),
                            chat_state::InputRoute::Steer {
                                target_turn: chat_state::TurnId(1),
                            },
                            Vec::new(),
                        )
                        .await
                        .is_err(),
                    "the same identity cannot change from FIFO to steer"
                );

                let (steer_actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_active_causal_turn(&steer_actor)
                    .await;
                let target_turn = steer_actor
                    .events
                    .current_turn()
                    .expect("test actor has an active durable turn");
                let mut original = input_inbox::tests::text_payload("steer-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut original;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "1".into(),
                };
                steer_actor
                    .admit_human_input(
                        chat_state::InputIntent::Steer,
                        original,
                        Some("steer-prompt".into()),
                        chat_state::InputRoute::Steer { target_turn },
                        Vec::new(),
                    )
                    .await
                    .expect("initial steer input admitted");

                let mut changed_target =
                    input_inbox::tests::text_payload("steer-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut changed_target;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "2".into(),
                };
                assert!(
                    steer_actor
                        .admit_human_input(
                            chat_state::InputIntent::Steer,
                            changed_target,
                            Some("steer-prompt".into()),
                            chat_state::InputRoute::Steer {
                                target_turn: chat_state::TurnId(2),
                            },
                            Vec::new(),
                        )
                        .await
                        .is_err(),
                    "the same identity cannot change its original steer target"
                );

                for actor in [&actor, &steer_actor] {
                    let events = actor.chat_state_handle.timeline_events().await.unwrap();
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| matches!(
                                &event.kind,
                                chat_state::TimelineEventKind::Hook(
                                    chat_state::HookEvent::Triggered {
                                        event: chat_state::HookEventType::UserPromptSubmit,
                                        ..
                                    }
                                )
                            ))
                            .count(),
                        1,
                        "route conflicts must be rejected before a second Hook"
                    );
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn same_text_with_a_new_identity_is_admitted_as_a_new_input() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("first-prompt", "same text"),
                        Some("first-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .unwrap();
                let second = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("second-prompt", "same text"),
                        Some("second-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .unwrap();

                assert_ne!(first.input_id, second.input_id);
                assert!(!first.replayed && !second.replayed);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    2
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                event: chat_state::HookEventType::UserPromptSubmit,
                                ..
                            })
                        ))
                        .count(),
                    2
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shared_storage_failure_rejects_queued_input_before_hook_and_keeps_observation_readable()
     {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let shared_gate =
                    std::sync::Arc::new(crate::session::storage_admission::StorageAdmission::new(
                        actor.session_dir.clone(),
                    ));
                let manager_clone = shared_gate.clone();
                manager_clone.record_failure(
                    actor.session_info.id.0.as_ref(),
                    "failed-writer",
                    None,
                    Some(std::io::ErrorKind::StorageFull),
                    "disk full",
                );
                std::sync::Arc::get_mut(&mut actor)
                    .expect("test actor has one owner")
                    .tool_context
                    .storage_admission = Some(shared_gate);

                let diagnostic_session_id = actor.runtime_observation.read().session_id.clone();
                let (respond_to, response) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        vec![acp::ContentBlock::Text(acp::TextContent::new(
                            "blocked input",
                        ))],
                        "blocked-prompt".into(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_to,
                        None,
                    )
                    .await;

                assert!(response.await.unwrap().is_err());
                assert_eq!(
                    actor.runtime_observation.read().session_id,
                    diagnostic_session_id,
                    "read-only runtime diagnostics remain available during storage failure"
                );
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted { .. })
                        | chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            event: chat_state::HookEventType::UserPromptSubmit,
                            ..
                        })
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_input_identity_replays_share_one_submission_and_hook() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor.admit_human_input(
                    chat_state::InputIntent::Prompt,
                    input_inbox::tests::text_payload("parallel-prompt", "same request"),
                    Some("parallel-prompt".into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                );
                let second = actor.admit_human_input(
                    chat_state::InputIntent::Prompt,
                    input_inbox::tests::text_payload("parallel-prompt", "same request"),
                    Some("parallel-prompt".into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                );
                let (first, second) = tokio::join!(first, second);
                let first = first.expect("first admission");
                let second = second.expect("concurrent retry");
                assert_eq!(first.input_id, second.input_id);
                assert_ne!(first.replayed, second.replayed);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                event: chat_state::HookEventType::UserPromptSubmit,
                                ..
                            })
                        ))
                        .count(),
                    1
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_input_recovery_reuses_admission_and_snapshot() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let input_id = admit_image(&actor).await;
                let before = actor.chat_state_handle.timeline_events().await.unwrap();
                actor.restore_pending_human_inputs().await.unwrap();
                let state = actor.state.lock().await;
                let restored = state.pending_inputs.front().unwrap();
                assert_eq!(restored.input_ids, vec![input_id]);
                let InputPayload::Prompt { prompt_blocks, .. } =
                    input_inbox::tests::image_payload();
                assert_eq!(
                    serde_json::to_value(&restored.prompt_blocks).unwrap(),
                    serde_json::to_value(prompt_blocks).unwrap()
                );
                drop(state);
                assert_eq!(
                    before.len(),
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    "recovery must not repeat Hooks or admission"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_input_attachment_is_invalidated_without_blocking_other_inputs() {
        tokio::task::LocalSet::new().run_until(async {
            let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
            let missing_id = admit_image(&actor).await;
            let image_dir = actor.session_dir.join("artifacts/inputs/images");
            for path in std::fs::read_dir(&image_dir).unwrap() {
                std::fs::remove_file(path.unwrap().path()).unwrap();
            }
            let mut text = input_inbox::tests::image_payload();
            let InputPayload::Prompt { prompt_blocks, prompt_id, .. } = &mut text;
            prompt_blocks.retain(|block| matches!(block, acp::ContentBlock::Text(_)));
            *prompt_id = "healthy".into();
            let healthy_id = actor.admit_human_input(chat_state::InputIntent::Prompt, text, Some("healthy".into()), chat_state::InputRoute::Fifo, vec![]).await.unwrap().input_id;
            actor.restore_pending_human_inputs().await.unwrap();
            let state = actor.state.lock().await;
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(state.pending_inputs.front().unwrap().input_ids, vec![healthy_id]);
            drop(state);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Dismissed { input_ids, reason: chat_state::InputDismissReason::Invalidated }) if input_ids == &vec![missing_id.clone()]
            )));
            assert_eq!(actor.chat_state_handle.submitted_input_payload_hashes().await.unwrap().len(), 2, "invalidated snapshots remain audit roots");
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_failure_retry_has_one_submission_and_gc_keeps_consumed_images() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let artifact_path = actor.session_dir.join("artifacts");
                std::fs::write(&artifact_path, b"fault injection").unwrap();
                let failure = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::image_payload(),
                        Some("prompt-1".into()),
                        chat_state::InputRoute::Fifo,
                        vec![],
                    )
                    .await;
                assert!(failure.is_err());
                assert!(
                    actor
                        .chat_state_handle
                        .submitted_input_payload_hashes()
                        .await
                        .unwrap()
                        .is_empty()
                );
                std::fs::remove_file(artifact_path).unwrap();
                let input_id = admit_image(&actor).await;
                let turn = chat_state::TurnId(1);
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Turn(
                        chat_state::TurnEvent::Started {
                            prompt_id: "fixture-prompt".into(),
                            id: turn,
                            input_ids: vec![input_id.clone()],
                            identity: crate::session::PromptOrigin::User
                                .turn_identity(crate::session::TurnKind::User),
                            model_id: "test".into(),
                            input_message_count: 0,
                            prompt_index: 0,
                            prompt_text: "image".into(),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            redirect_kind: None,
                        },
                    ))
                    .await
                    .unwrap();
                actor
                    .consume_fifo_inputs(
                        vec![input_id],
                        turn,
                        sampling_types::ConversationItem::user("consumed image"),
                        None,
                    )
                    .await
                    .unwrap();
                assert!(
                    actor
                        .chat_state_handle
                        .get_pending_allowed_inputs()
                        .await
                        .unwrap()
                        .is_empty()
                );
                actor
                    .reconcile_input_payloads(&tokio_util::sync::CancellationToken::new())
                    .await;
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let refs = events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted { payload_ref, .. },
                        ) => Some(payload_ref),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(refs.len(), 1);
                assert_eq!(
                    input_inbox::read_payload(&actor.session_directory, refs[0])
                        .unwrap()
                        .hook_prompt(),
                    input_inbox::tests::image_payload().hook_prompt()
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stable_fifo_retry_observes_canonical_state_across_derived_intents() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let payload =
                    input_inbox::tests::text_payload("fifo-intent-derivation", "same request");
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some("fifo-intent-derivation".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("idle admission");
                assert!(!first.replayed);
                // The server re-derives Prompt vs Followup from idle/running state;
                // that derivation is not a user intent change and must not conflict.
                let second = actor
                    .admit_human_input(
                        chat_state::InputIntent::Followup,
                        payload.clone(),
                        Some("fifo-intent-derivation".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("derived-intent retry is an observation");
                assert!(second.replayed);
                assert_eq!(second.input_id, first.input_id);
                assert_eq!(
                    second.replay_state,
                    Some(chat_state::InputState::Allowed {
                        route: chat_state::InputRoute::Fifo,
                    })
                );
                // Reverse direction: first admitted as Followup, retried as Prompt.
                let reverse_payload =
                    input_inbox::tests::text_payload("fifo-intent-reverse", "reverse");
                actor
                    .admit_human_input(
                        chat_state::InputIntent::Followup,
                        reverse_payload.clone(),
                        Some("fifo-intent-reverse".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("followup admission");
                let reverse_retry = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        reverse_payload,
                        Some("fifo-intent-reverse".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("prompt retry is an observation");
                assert!(reverse_retry.replayed);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            ) | chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered { .. }
                            )
                        ))
                        .count(),
                    4,
                    "two admissions only: one Submitted plus one Hook each, retries add none"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stable_retry_returns_each_canonical_state_as_observation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;

                // Submitted: durable submission without a resolved admission.
                let submitted_payload =
                    input_inbox::tests::text_payload("state-submitted", "pending hook");
                let payload_ref =
                    input_inbox::write_payload(&actor.session_directory, &submitted_payload)
                        .unwrap();
                let submitted_id = input_inbox::input_id_for_prompt(
                    actor.session_info.id.0.as_ref(),
                    "state-submitted",
                );
                actor
                    .chat_state_handle
                    .submit_input_durably(
                        submitted_id.clone(),
                        chat_state::InputIntent::Prompt,
                        payload_ref,
                    )
                    .await
                    .unwrap();
                let submitted_replay = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        submitted_payload,
                        Some("state-submitted".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("submitted retry observes the canonical state");
                assert_eq!(submitted_replay.input_id, submitted_id);
                assert_eq!(
                    submitted_replay.replay_state,
                    Some(chat_state::InputState::Submitted)
                );

                // Blocked, Handled and Dismissed (including RouteSuperseded) all
                // replay as observations of their durable state.
                for prompt_id in [
                    "state-blocked",
                    "state-handled",
                    "state-dismissed",
                    "state-superseded",
                ] {
                    let payload = input_inbox::tests::text_payload(prompt_id, "terminal states");
                    let input_id = if prompt_id == "state-blocked" {
                        // A durable submission resolves to Blocked; an
                        // Allowed admission is terminal and cannot be
                        // re-folded into Blocked, so the blocked fixture is
                        // driven straight from submission to denial.
                        let payload_ref =
                            input_inbox::write_payload(&actor.session_directory, &payload).unwrap();
                        let input_id = input_inbox::input_id_for_prompt(
                            actor.session_info.id.0.as_ref(),
                            prompt_id,
                        );
                        actor
                            .chat_state_handle
                            .submit_input_durably(
                                input_id.clone(),
                                chat_state::InputIntent::Prompt,
                                payload_ref,
                            )
                            .await
                            .unwrap();
                        // The causal fold derives an admission resolution
                        // from a completed Hook occurrence; with no declared
                        // handlers the aggregate is Allow, so the blocked
                        // fixture records the same durable facts
                        // recover_interrupted produces: an empty-handler
                        // completed hook resolving to Block with the
                        // ProcessInterrupted reason.
                        let occurrence_id = format!("{prompt_id}-denied-hook");
                        actor
                            .chat_state_handle
                            .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    occurrence_id: occurrence_id.clone(),
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    gate: chat_state::HookGateKind::Prompt,
                                    cause: chat_state::HookCause::Input {
                                        input_id: input_id.clone(),
                                    },
                                    config_generation: 0,
                                    handlers: Vec::new(),
                                },
                            ))
                            .await
                            .unwrap();
                        actor
                            .chat_state_handle
                            .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Completed {
                                    occurrence_id: occurrence_id.clone(),
                                    decision: chat_state::HookAggregateDecision::Prompt {
                                        decision: chat_state::HookGateDecision::Allow,
                                    },
                                },
                            ))
                            .await
                            .unwrap();
                        actor
                            .record_input_event(chat_state::InputEvent::AdmissionResolved {
                                input_id: input_id.clone(),
                                decision: chat_state::InputAdmissionDecision::Block {
                                    reason: chat_state::InputBlockReason::ProcessInterrupted,
                                },
                                route: None,
                                supersedes: Vec::new(),
                            })
                            .await
                            .unwrap();
                        input_id
                    } else {
                        let admitted = actor
                            .admit_human_input(
                                chat_state::InputIntent::Prompt,
                                payload.clone(),
                                Some(prompt_id.into()),
                                chat_state::InputRoute::Fifo,
                                Vec::new(),
                            )
                            .await
                            .unwrap();
                        match prompt_id {
                            "state-handled" => {
                                // Handled folds from a real FIFO turn: the
                                // validator requires the input to be the
                                // turn's reserved input while the turn is
                                // active, so the fixture records the same
                                // durable facts a completed turn produces.
                                let handled_turn = chat_state::TurnId(1);
                                actor
                                    .chat_state_handle
                                    .record_timeline_event_durably(
                                        chat_state::TimelineEventKind::Turn(
                                            chat_state::TurnEvent::Started {
                                                prompt_id: "state-handled-prompt".into(),
                                                id: handled_turn,
                                                input_ids: vec![admitted.input_id.clone()],
                                                identity: chat_state::TurnIdentity {
                                                    origin: "user".into(),
                                                    turn_kind: "user".into(),
                                                    goal_id: None,
                                                    goal_definition_revision: None,
                                                    stage_id: None,
                                                },
                                                model_id: "test-model".into(),
                                                input_message_count: 0,
                                                prompt_index: 0,
                                                prompt_text: "terminal states".into(),
                                                input_kind: chat_state::TurnInputKind::Prompt,
                                                redirect_kind: None,
                                            },
                                        ),
                                    )
                                    .await
                                    .unwrap();
                                actor
                                    .complete_unmodeled_fifo_inputs(
                                        vec![admitted.input_id.clone()],
                                        handled_turn,
                                    )
                                    .await
                                    .unwrap();
                                // End the turn so the remaining terminal
                                // states stay independent of the active
                                // turn boundary.
                                actor
                                    .chat_state_handle
                                    .record_timeline_event_durably(
                                        chat_state::TimelineEventKind::Turn(
                                            chat_state::TurnEvent::Ended {
                                                id: handled_turn,
                                                outcome: "completed".into(),
                                                duration_ms: 1,
                                                tool_count: 0,
                                                terminal: chat_state::TurnTerminal {
                                                    source: chat_state::TurnTerminalSource::Host,
                                                    stop_reason: "end_turn".into(),
                                                    completion_kind: "completed".into(),
                                                },
                                                cancellation_category: None,
                                                details: None,
                                            },
                                        ),
                                    )
                                    .await
                                    .unwrap();
                            }
                            "state-dismissed" => {
                                actor
                                    .dismiss_input_ids(
                                        vec![admitted.input_id.clone()],
                                        chat_state::InputDismissReason::UserRemoved,
                                    )
                                    .await
                                    .unwrap();
                            }
                            "state-superseded" => {
                                actor
                                    .dismiss_input_ids(
                                        vec![admitted.input_id.clone()],
                                        chat_state::InputDismissReason::RouteSuperseded,
                                    )
                                    .await
                                    .unwrap();
                            }
                            _ => unreachable!(),
                        }
                        admitted.input_id
                    };
                    let replay = actor
                        .admit_human_input(
                            chat_state::InputIntent::Prompt,
                            payload,
                            Some(prompt_id.into()),
                            chat_state::InputRoute::Fifo,
                            Vec::new(),
                        )
                        .await
                        .unwrap_or_else(|error| panic!("{prompt_id} retry must observe: {error}"));
                    assert_eq!(replay.input_id, input_id, "{prompt_id}");
                    assert!(replay.replayed, "{prompt_id}");
                    let expected = match prompt_id {
                        "state-blocked" => Some(chat_state::InputState::Blocked {
                            reason: chat_state::InputBlockReason::ProcessInterrupted,
                        }),
                        "state-handled" => Some(chat_state::InputState::Handled {
                            route: chat_state::InputRoute::Fifo,
                        }),
                        "state-dismissed" => Some(chat_state::InputState::Dismissed {
                            reason: chat_state::InputDismissReason::UserRemoved,
                        }),
                        "state-superseded" => Some(chat_state::InputState::Dismissed {
                            reason: chat_state::InputDismissReason::RouteSuperseded,
                        }),
                        _ => unreachable!(),
                    };
                    assert_eq!(replay.replay_state, expected, "{prompt_id}");
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stable_retry_conflicts_on_changed_payload_or_explicit_steer() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let payload =
                    input_inbox::tests::text_payload("conflict-prompt", "original request");
                let admitted = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some("conflict-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .unwrap();
                let changed = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("conflict-prompt", "changed request"),
                        Some("conflict-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect_err("changed payload under one identity must conflict");
                assert!(
                    changed.contains("immutable payload or route differs"),
                    "unexpected conflict message: {changed}"
                );
                let mut steer_payload =
                    input_inbox::tests::text_payload("conflict-prompt", "original request");
                if let input_inbox::InputPayload::Prompt {
                    requested_route, ..
                } = &mut steer_payload
                {
                    *requested_route = input_inbox::InputRequestRoute::Steer {
                        expected_turn_id: "conflict-prompt".into(),
                    };
                }
                let steer = actor
                    .admit_human_input(
                        chat_state::InputIntent::Steer,
                        steer_payload,
                        Some("conflict-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect_err("explicit steer under a FIFO identity must conflict");
                assert!(
                    steer.contains("conflict"),
                    "unexpected steer conflict message: {steer}"
                );
                // The original input is untouched and the conflicts admitted nothing.
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    1,
                    "no new Submitted facts for conflicting retries"
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .input_state(&admitted.input_id)
                        .await
                        .unwrap()
                        .unwrap()
                        .state,
                    chat_state::InputState::Allowed {
                        route: chat_state::InputRoute::Fifo,
                    }
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queue_input_retry_returns_admission_replay_once() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let prompt_id = "queue-retry-prompt".to_string();
                let blocks = || {
                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                        "queue retry request",
                    ))]
                };
                let (respond_tx, respond_rx) = tokio::sync::oneshot::channel();
                let (ack_tx, mut ack_rx) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        blocks(),
                        prompt_id.clone(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_tx,
                        Some(ack_tx),
                    )
                    .await;
                assert!(
                    ack_rx.try_recv().is_err(),
                    "first submission is not a replay"
                );
                let (retry_tx, retry_rx) = tokio::sync::oneshot::channel();
                let (retry_ack_tx, retry_ack_rx) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        blocks(),
                        prompt_id.clone(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        retry_tx,
                        Some(retry_ack_tx),
                    )
                    .await;
                let replay = retry_rx
                    .await
                    .expect("retry RPC resolves as an admission replay")
                    .expect("replay is not an error");
                assert_eq!(replay.stop_reason, acp::StopReason::EndTurn);
                assert_eq!(replay.total_tokens, 0);
                assert!(replay.usage.is_none() && replay.structured_output.is_none());
                let expected_input_id =
                    input_inbox::input_id_for_prompt(actor.session_info.id.0.as_ref(), &prompt_id);
                let completion_kind = match replay.completion_kind {
                    crate::session::commands::PromptCompletionKind::AdmissionReplay {
                        input_id,
                        state,
                    } => {
                        assert_eq!(input_id, expected_input_id);
                        state
                    }
                    other => panic!("retry must not fabricate a turn terminal: {other:?}"),
                };
                assert_eq!(
                    completion_kind,
                    chat_state::InputState::Allowed {
                        route: chat_state::InputRoute::Fifo,
                    }
                );
                // The persist acknowledgement is returned after the observation.
                retry_ack_rx
                    .await
                    .expect("retry returns the original persist acknowledgement");
                // The queue holds exactly one entry; the retry neither requeued nor
                // removed the original row, and the first RPC is still pending.
                assert_eq!(actor.state.lock().await.pending_inputs.len(), 1);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            ) | chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered { .. }
                            )
                        ))
                        .count(),
                    2,
                    "hook and submission each ran exactly once"
                );
                drop(respond_rx);
            })
            .await;
    }

    struct DurableQueueSession {
        root: tempfile::TempDir,
        storage: crate::session::storage::jsonl::JsonlStorageAdapter,
        info: crate::session::info::Info,
    }

    /// A real JSONL session whose seed Timeline is already durable on disk.
    /// Every later accepted admission appends through the same adapter, so a
    /// cold load reads exactly the acknowledged facts.
    async fn durable_queue_session() -> DurableQueueSession {
        use crate::session::storage::StorageAdapter as _;

        let root = tempfile::tempdir().expect("durable session root");
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
            root.path().to_path_buf(),
        );
        let info = crate::session::info::Info {
            id: acp::SessionId::new("test-actor"),
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
        };
        storage
            .init_session(&info, crate::session::persistence::default_model_id())
            .await
            .expect("initialize the durable session");
        let seed = chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::system(
            "test system prompt",
        )])
        .expect("seed Timeline");
        for event in seed.events() {
            storage
                .append_timeline_event_durable(&info, event)
                .await
                .expect("seed durable Timeline");
        }
        DurableQueueSession {
            root,
            storage,
            info,
        }
    }

    fn durable_sampling_config() -> sampling_types::SamplingConfig {
        sampling_types::SamplingConfig {
            base_url: "http://localhost".into(),
            model: "test".into(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(256_000).unwrap(),
            reasoning_effort: None,
            stream_tool_calls: None,
        }
    }

    /// Wire a test SessionActor to a ChatState owner that appends every
    /// accepted event to the real JSONL session, replacing the in-memory test
    /// owner entirely.
    async fn actor_with_durable_chat_state(
        storage: &crate::session::storage::jsonl::JsonlStorageAdapter,
        info: &crate::session::info::Info,
        timeline: chat_state::Timeline,
    ) -> std::sync::Arc<SessionActor> {
        use crate::session::storage::StorageAdapter as _;

        let directory = storage
            .open_session(info)
            .expect("open the durable session")
            .directory_handle();
        let directory_path = directory.display_path().to_path_buf();
        let (mut actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
        let chat_state = chat_state::ChatStateActor::spawn_from_validated_timeline(
            timeline,
            durable_sampling_config(),
            None,
            Box::new(JsonlTimelinePersistence {
                storage: storage.clone(),
                info: info.clone(),
            }),
            tokio::sync::mpsc::unbounded_channel().0,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("durable ChatState owner");
        let inner = std::sync::Arc::get_mut(&mut actor).expect("single test actor owner");
        inner.session_info = info.clone();
        inner.session_dir = directory_path;
        inner.session_directory = directory;
        inner.events = crate::session::events::EventTracker::new(chat_state.clone());
        inner.chat_state_handle = chat_state;
        actor
    }

    async fn queue_human_row(
        actor: &SessionActor,
        queue_id: &str,
        blocks: Vec<acp::ContentBlock>,
        client_identifier: Option<&str>,
        screen_mode: Option<&str>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
    ) -> String {
        let (respond_to, _response) = tokio::sync::oneshot::channel();
        actor
            .queue_input(
                blocks,
                queue_id.to_string(),
                crate::session::PromptOrigin::User,
                crate::session::TurnKind::User,
                client_identifier.map(str::to_owned),
                screen_mode.map(str::to_owned),
                verbatim,
                json_schema,
                respond_to,
                None,
            )
            .await;
        actor
            .state
            .lock()
            .await
            .pending_inputs
            .iter()
            .find(|item| {
                item.queue_meta
                    .as_ref()
                    .is_some_and(|meta| meta.id == queue_id)
            })
            .and_then(|item| item.input_ids.first().cloned())
            .expect("admitted queue row")
    }

    async fn save_queue_edit(
        actor: &SessionActor,
        id: &str,
        expected_version: u64,
        edit_id: &str,
        text: &str,
        leader_client_id: u64,
    ) -> String {
        use crate::session::prompt_queue::{QueueControlOperation, QueueControlRequest};

        let request = |operation, new_text: Option<String>| QueueControlRequest {
            session_id: actor.session_id_string(),
            operation,
            id: id.to_string(),
            expected_version,
            edit_id: Some(edit_id.to_string()),
            new_text,
            leader_client_id: Some(leader_client_id),
        };
        let hold = actor
            .handle_queue_control(request(QueueControlOperation::Hold, None))
            .await;
        assert!(hold.applied, "hold rejected: {:?}", hold.reason);
        let saved = actor
            .handle_queue_control(request(QueueControlOperation::Save, Some(text.to_string())))
            .await;
        assert!(saved.applied, "save rejected: {:?}", saved.reason);
        assert_eq!(saved.version, Some(expected_version + 1));
        actor
            .state
            .lock()
            .await
            .pending_inputs
            .iter()
            .find(|item| item.queue_meta.as_ref().is_some_and(|meta| meta.id == id))
            .and_then(|item| item.input_ids.first().cloned())
            .expect("edited queue row")
    }

    fn row_text(item: &InputItem) -> String {
        item.prompt_blocks
            .iter()
            .find_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .expect("queue row has prompt text")
    }

    /// Cold-restore identity for an edited FIFO row: a real JSONL session,
    /// three admitted rows, two consecutive edits of A and one edit of B, then
    /// the live owner is retired and rebuilt from the acknowledged file.
    /// Restoration must reuse the durable row id as the runtime prompt id,
    /// read the newest immutable payload for content, and leave the replacement
    /// admission identity untouched.
    #[tokio::test(flavor = "current_thread")]
    async fn edited_queue_rows_restore_row_identity_from_latest_immutable_payload() {
        tokio::task::LocalSet::new()
            .run_until(async {
                use crate::session::storage::StorageAdapter as _;

                let DurableQueueSession {
                    root,
                    storage,
                    info,
                } = durable_queue_session().await;
                let timeline = storage
                    .open_session(&info)
                    .expect("open seeded session")
                    .validated_timeline(&info.id.to_string())
                    .expect("validated seed")
                    .timeline;
                let actor = actor_with_durable_chat_state(&storage, &info, timeline).await;

                let image = crate::session::actor::tests::support::test_image_content();
                let a_original = queue_human_row(
                    &actor,
                    "queue-a",
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("alpha")),
                        acp::ContentBlock::Image(image.clone()),
                    ],
                    Some("client-a"),
                    Some("screen-a"),
                    false,
                    None,
                )
                .await;
                let b_original = queue_human_row(
                    &actor,
                    "queue-b",
                    vec![acp::ContentBlock::Text(acp::TextContent::new("bravo"))],
                    Some("client-b"),
                    None,
                    true,
                    None,
                )
                .await;
                let c_original = queue_human_row(
                    &actor,
                    "queue-c",
                    vec![acp::ContentBlock::Text(acp::TextContent::new("charlie"))],
                    None,
                    None,
                    false,
                    Some(serde_json::json!({"type": "object"})),
                )
                .await;

                let a_edit_one =
                    save_queue_edit(&actor, "queue-a", 0, "edit-a-1", "alpha edit one", 1).await;
                let a_edit_two =
                    save_queue_edit(&actor, "queue-a", 1, "edit-a-2", "alpha edit two", 1).await;
                let b_edit =
                    save_queue_edit(&actor, "queue-b", 0, "edit-b-1", "bravo edited", 2).await;

                // Live rows keep the original queue/runtime identity while the
                // newest immutable payload carries the replacement identity.
                {
                    let state = actor.state.lock().await;
                    let row = state
                        .pending_inputs
                        .iter()
                        .find(|item| item.queue_meta.as_ref().is_some_and(|m| m.id == "queue-a"))
                        .expect("live edited row");
                    assert_eq!(row.prompt_id, "queue-a");
                    assert_eq!(row.input_ids, vec![a_edit_two.clone()]);
                    assert_eq!(row.queue_meta.as_ref().unwrap().version, 2);
                    assert_eq!(row.queue_meta.as_ref().unwrap().text, "alpha edit two");
                    assert_eq!(row_text(row), "alpha edit two");
                    assert_eq!(row.prompt_blocks.len(), 2, "edited row keeps its image");
                }
                let live_pending = actor
                    .chat_state_handle
                    .get_pending_allowed_inputs()
                    .await
                    .expect("canonical pending query");
                assert_eq!(
                    live_pending
                        .iter()
                        .map(|pending| pending.input_id.as_str())
                        .collect::<Vec<_>>(),
                    vec![a_edit_two.as_str(), b_edit.as_str(), c_original.as_str()],
                    "the replacement inherits A's canonical FIFO slot"
                );
                let live_payload_ids = live_pending
                    .iter()
                    .map(|pending| {
                        input_inbox::read_prompt_id(
                            actor.session_directory.as_ref(),
                            &pending.payload_ref,
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    live_payload_ids,
                    vec!["queue-a:edit:3", "queue-b:edit:2", "queue-c"]
                );
                let live_events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    live_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    6,
                    "A twice, B once and three originals were each admitted"
                );

                // Retire the whole live owner, then rebuild from the file. The
                // writer lease lives in the adapter, so both the actor and the
                // local handle must be gone before the cold load.
                let previous = std::sync::Arc::downgrade(&actor);
                drop(actor);
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while previous.upgrade().is_some() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the live actor and its persistence clone retired");
                drop(storage);
                let cold_storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
                    root.path().to_path_buf(),
                );
                let loaded = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        match cold_storage
                            .load_session_for_write_without_updates(&info)
                            .await
                        {
                            Ok(loaded) => break loaded,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            }
                            Err(error) => panic!("cold writer load failed: {error}"),
                        }
                    }
                })
                .await
                .expect("cold writer load");
                assert_eq!(
                    loaded.timeline.events().len(),
                    live_events.len(),
                    "the cold Timeline contains every acknowledged admission"
                );

                let restored_actor =
                    actor_with_durable_chat_state(&cold_storage, &info, loaded.timeline).await;
                let before_restore = restored_actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap();
                restored_actor
                    .restore_pending_human_inputs()
                    .await
                    .expect("cold input reconstruction");
                let after_restore = restored_actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap();
                assert_eq!(
                    after_restore.len(),
                    before_restore.len(),
                    "reconstruction records nothing new"
                );
                for input_id in [
                    &a_original,
                    &a_edit_one,
                    &a_edit_two,
                    &b_original,
                    &b_edit,
                    &c_original,
                ] {
                    assert_eq!(
                        after_restore
                            .iter()
                            .filter(|event| matches!(
                                &event.kind,
                                chat_state::TimelineEventKind::Hook(
                                    chat_state::HookEvent::Triggered {
                                        event: chat_state::HookEventType::UserPromptSubmit,
                                        cause: chat_state::HookCause::Input { input_id: cause },
                                        ..
                                    }
                                ) if cause == input_id
                            ))
                            .count(),
                        1,
                        "cold reconstruction must not replay the admission Hook for {input_id}"
                    );
                }

                {
                    let state = restored_actor.state.lock().await;
                    assert_eq!(state.pending_inputs.len(), 3);
                    let rows = state.pending_inputs.iter().collect::<Vec<_>>();
                    assert_eq!(
                        rows.iter()
                            .map(|row| (
                                row.prompt_id.as_str(),
                                row.queue_meta
                                    .as_ref()
                                    .map(|meta| (meta.id.as_str(), meta.version)),
                                row.input_ids.clone(),
                            ))
                            .collect::<Vec<_>>(),
                        vec![
                            ("queue-a", Some(("queue-a", 2)), vec![a_edit_two.clone()]),
                            ("queue-b", Some(("queue-b", 1)), vec![b_edit.clone()]),
                            ("queue-c", Some(("queue-c", 0)), vec![c_original.clone()]),
                        ],
                        "restored rows keep canonical order, row identity and newest input ids"
                    );
                    let a_row = rows[0];
                    assert_eq!(a_row.queue_meta.as_ref().unwrap().text, "alpha edit two");
                    assert_eq!(row_text(a_row), "alpha edit two");
                    assert_eq!(a_row.client_identifier.as_deref(), Some("client-a"));
                    assert_eq!(a_row.screen_mode.as_deref(), Some("screen-a"));
                    assert_eq!(a_row.prompt_blocks.len(), 2);
                    assert_eq!(a_row.prompt_blocks[1], acp::ContentBlock::Image(image));
                    let b_row = rows[1];
                    assert_eq!(row_text(b_row), "bravo edited");
                    assert!(b_row.verbatim);
                    assert_eq!(b_row.client_identifier.as_deref(), Some("client-b"));
                    let c_row = rows[2];
                    assert_eq!(row_text(c_row), "charlie");
                    assert_eq!(
                        c_row.json_schema,
                        Some(serde_json::json!({"type": "object"}))
                    );
                }

                let restored_pending = restored_actor
                    .chat_state_handle
                    .get_pending_allowed_inputs()
                    .await
                    .expect("restored canonical pending query");
                assert_eq!(
                    restored_pending
                        .iter()
                        .map(|pending| pending.input_id.as_str())
                        .collect::<Vec<_>>(),
                    vec![a_edit_two.as_str(), b_edit.as_str(), c_original.as_str()]
                );
                let restored_payload_ids = restored_pending
                    .iter()
                    .map(|pending| {
                        input_inbox::read_prompt_id(
                            restored_actor.session_directory.as_ref(),
                            &pending.payload_ref,
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    restored_payload_ids,
                    vec!["queue-a:edit:3", "queue-b:edit:2", "queue-c"],
                    "the newest immutable payload, not the original request, is read"
                );
                let restored_payload = input_inbox::read_payload(
                    restored_actor.session_directory.as_ref(),
                    &restored_pending[0].payload_ref,
                )
                .unwrap();
                let InputPayload::Prompt {
                    prompt_id,
                    prompt_blocks,
                    queue,
                    ..
                } = restored_payload;
                assert_eq!(prompt_id, "queue-a:edit:3");
                let queue = queue.expect("edited row keeps its stored queue entry");
                assert_eq!((queue.id.as_str(), queue.version), ("queue-a", 2));
                assert_eq!(queue.text, "alpha edit two");
                assert_eq!(prompt_blocks.len(), 2);

                for superseded in [&a_original, &a_edit_one, &b_original] {
                    assert_eq!(
                        restored_actor
                            .chat_state_handle
                            .input_state(superseded)
                            .await
                            .unwrap()
                            .map(|lifecycle| lifecycle.state),
                        Some(chat_state::InputState::Dismissed {
                            reason: chat_state::InputDismissReason::RouteSuperseded,
                        }),
                        "replaced admission {superseded} stays RouteSuperseded"
                    );
                }
                assert_eq!(
                    restored_actor
                        .chat_state_handle
                        .input_state(&a_edit_two)
                        .await
                        .unwrap()
                        .map(|lifecycle| lifecycle.state),
                    Some(chat_state::InputState::Allowed {
                        route: chat_state::InputRoute::Fifo,
                    })
                );

                // The restored row is consumable exactly once through the real
                // durable entry; the replacement input id, not the old row,
                // owns the consumption.
                let turn = chat_state::TurnId(7);
                restored_actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Turn(
                        chat_state::TurnEvent::Started {
                            prompt_id: "queue-a".into(),
                            id: turn,
                            input_ids: vec![a_edit_two.clone()],
                            identity: chat_state::TurnIdentity {
                                origin: "user".into(),
                                turn_kind: "user".into(),
                                goal_id: None,
                                goal_definition_revision: None,
                                stage_id: None,
                            },
                            model_id: "test".into(),
                            input_message_count: 1,
                            prompt_index: 0,
                            prompt_text: "alpha edit two".into(),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            redirect_kind: None,
                        },
                    ))
                    .await
                    .expect("durable user turn reserves the restored row");
                restored_actor
                    .consume_fifo_inputs(
                        vec![a_edit_two.clone()],
                        turn,
                        sampling_types::ConversationItem::user("alpha edit two"),
                        None,
                    )
                    .await
                    .expect("consume the restored row once");
                let pending_after = restored_actor
                    .chat_state_handle
                    .get_pending_allowed_inputs()
                    .await
                    .unwrap();
                assert_eq!(
                    pending_after
                        .iter()
                        .map(|pending| pending.input_id.as_str())
                        .collect::<Vec<_>>(),
                    vec![b_edit.as_str(), c_original.as_str()]
                );
                assert_eq!(
                    restored_actor
                        .chat_state_handle
                        .input_state(&a_edit_two)
                        .await
                        .unwrap()
                        .map(|lifecycle| lifecycle.state),
                    Some(chat_state::InputState::Consumed {
                        route: chat_state::InputRoute::Fifo,
                    })
                );
                assert_eq!(
                    after_restore
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Consumed { input_ids, .. }
                            ) if input_ids == &vec![a_edit_two.clone()]
                        ))
                        .count(),
                    0,
                    "cold reconstruction never consumes on its own"
                );
            })
            .await;
    }
}
