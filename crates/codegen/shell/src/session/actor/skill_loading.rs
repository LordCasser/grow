//! Explicit user references execute the registered loader before model sampling.
//! The input owner publishes the whole group in its existing consumption fact.
use super::*;
use sampling_types::{SkillLoadItem, SkillLoadSource};
use tools::implementations::grow_build::load_skill::{
    LoadSkillInput, LoadSkillOutput, SkillLoadSelection,
};

tokio::task_local! {
    /// Host context for the ordinary tool preflight. It cannot be supplied in
    /// model arguments and lasts only for this exact Runtime invocation.
    pub(super) static USER_SKILL_SELECTION: SkillLoadSelection;
}

pub(super) fn user_skill_selection() -> Option<SkillLoadSelection> {
    USER_SKILL_SELECTION.try_with(Clone::clone).ok()
}

pub(super) struct PreparedUserSkills {
    pub items: Vec<SkillLoadItem>,
    outputs: Vec<LoadSkillOutput>,
}

impl PreparedUserSkills {
    fn empty() -> Self {
        Self {
            items: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

impl SessionActor {
    pub(super) async fn finish_model_skill_load(
        &self,
        prepared: &PreparedToolCall,
        result: Result<ToolRunResult, tool_runtime::ToolError>,
    ) -> Result<(), acp::Error> {
        let prepared_result = match result {
            Ok(result) => match result.output {
                ToolsToolOutput::Dynamic(output) => {
                    serde_json::from_value::<LoadSkillOutput>(output.value)
                        .map_err(|error| error.to_string())
                }
                _ => Err("Skill loader returned an invalid output".into()),
            },
            Err(error) => Err(error.to_string()),
        };
        let mut published = false;
        let mut output = None;
        let mut failure = None;
        match prepared_result {
            Ok(loaded) => {
                // Match the actual arguments, including the advertised alias,
                // instead of letting directory display names change the call.
                let input: LoadSkillInput = serde_json::from_value(prepared.parsed_args.clone())?;
                let skill = SkillLoadItem {
                    input_id: format!("model:{}", prepared.call_id),
                    reference_index: 0,
                    call_id: prepared.call_id.clone(),
                    skill_name: input.name,
                    source: SkillLoadSource::Model,
                    args: input.args,
                    prepared_body: Arc::from(loaded.prepared_body.as_str()),
                    prompt_index: Some(self.current_turn_number.get() as usize),
                };
                let item = ConversationItem::tool_result(
                    prepared.call_id.clone(),
                    sampling_types::skill_load_result_content(&skill),
                );
                let revision = self
                    .chat_state_handle
                    .get_surface_revision()
                    .await
                    .ok_or_else(|| {
                        acp::Error::internal_error().data("Skill Surface revision unavailable")
                    })?;
                let window = self
                    .chat_state_handle
                    .get_sampling_config()
                    .await
                    .map(|config| config.context_window.get())
                    .unwrap_or(0);
                let rejected = "Skill body was prepared but was not loaded: current context changed or has insufficient headroom. No skill guidance from this call was published.";
                let outcome = self
                    .chat_state_handle
                    .publish_model_skill_conditionally(
                        item,
                        skill,
                        ConversationItem::tool_result(prepared.call_id.clone(), rejected),
                        revision,
                        super::context_recall::context_recall_max_context_tokens(window),
                        window,
                    )
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
                published = outcome == chat_state::ConditionalToolResultOutcome::Accepted;
                if published {
                    output = Some(loaded);
                } else {
                    failure = Some(format!("{rejected} ({outcome:?})"));
                }
            }
            Err(error) => {
                self.chat_state_handle
                    .push_tool_result_durably(ConversationItem::tool_result(
                        prepared.call_id.clone(),
                        format!("Skill was not loaded: {error}"),
                    ))
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
                failure = Some(error);
            }
        }
        self.events
            .tool_completed_durably(
                &prepared.call_id,
                if published { "completed" } else { "failed" }.into(),
                Some(json!({"published":published})),
            )
            .await
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        let raw_output = match &output {
            Some(output) => serde_json::to_value(ToolsToolOutput::from(output.clone()))?,
            None => json!({"error":"skill_not_loaded","message":failure}),
        };
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                prepared.tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(Some(if published {
                        acp::ToolCallStatus::Completed
                    } else {
                        acp::ToolCallStatus::Failed
                    }))
                    .raw_output(Some(raw_output.clone())),
            )),
            None,
        )
        .await;
        let (tool_input, tool_input_truncated) =
            ::hooks::event::truncate_payload(prepared.parsed_args.clone());
        if published {
            self.signals_handle()
                .record_tool_success(&prepared.tool_name);
            let (tool_result, tool_result_truncated) = ::hooks::event::truncate_payload(raw_output);
            self.dispatch_observe_hook(
                ::hooks::event::HookEventName::PostToolUse,
                chat_state::HookCause::Tool {
                    call_id: prepared.call_id.clone(),
                },
                ::hooks::event::HookPayload::PostToolUse {
                    tool_name: prepared.tool_name.clone(),
                    tool_use_id: prepared.call_id.clone(),
                    tool_input,
                    tool_input_truncated,
                    tool_result,
                    tool_result_truncated,
                    duration_ms: None,
                    is_backgrounded: false,
                    subagent_type: self.subagent_type_label(),
                },
                None,
            )
            .await
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        } else {
            self.signals_handle()
                .record_tool_failure(&prepared.tool_name);
            self.dispatch_observe_hook(
                ::hooks::event::HookEventName::PostToolUseFailure,
                chat_state::HookCause::Tool {
                    call_id: prepared.call_id.clone(),
                },
                ::hooks::event::HookPayload::PostToolUseFailure {
                    tool_name: prepared.tool_name.clone(),
                    tool_use_id: prepared.call_id.clone(),
                    tool_input,
                    tool_input_truncated,
                    error: failure.unwrap_or_else(|| "Skill was not loaded".into()),
                    subagent_type: self.subagent_type_label(),
                },
                None,
            )
            .await
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        }
        Ok(())
    }
    pub(super) async fn prepare_user_skill_loads(
        &self,
        input_ids: &[String],
        prompt_index: usize,
    ) -> Result<PreparedUserSkills, acp::Error> {
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or_else(|| acp::Error::internal_error().data("Skill input history unavailable"))?;
        let mut references = Vec::new();
        for input_id in input_ids {
            let payload_ref = events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                        input_id: id,
                        payload_ref,
                        ..
                    }) if id == input_id => Some(payload_ref),
                    _ => None,
                })
                .ok_or_else(|| {
                    acp::Error::internal_error().data("Skill input has no immutable payload")
                })?;
            let payload =
                crate::session::input_inbox::read_payload(&self.session_directory, payload_ref)
                    .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            let crate::session::input_inbox::InputPayload::Prompt {
                skill_invocations, ..
            } = payload;
            for (index, reference) in skill_invocations.into_iter().enumerate() {
                references.push((input_id.clone(), index, reference));
            }
        }
        if references.is_empty() {
            return Ok(PreparedUserSkills::empty());
        }
        if self.events.has_active_step() {
            let boundary = self
                .end_step_control_boundary("interjection_skills")
                .await
                .ok_or_else(|| {
                    acp::Error::internal_error()
                        .data("The current Step could not close before skill preparation")
                })?;
            self.apply_pending_controls_at_step_boundary(boundary).await;
        }
        self.events
            .emit(crate::session::events::Event::LoopStarted {
                loop_index: self.events.next_step_index(),
            });
        let mut group = PreparedUserSkills::empty();
        let mut bytes = 0usize;
        let mut open_call = None;
        let result = async {
            for (input_id, reference_index, reference) in references {
                if let Some(error) = reference.selection_error {
                    return Err(acp::Error::invalid_params().data(error));
                }
                let selected = reference.skill.ok_or_else(|| {
                    acp::Error::invalid_params().data("Skill selection is unavailable")
                })?;
                let input = LoadSkillInput {
                    // `name` is the exact callable token the slash resolver
                    // accepted (bare when unique, qualified on collision).
                    // `qualified_name` is the stable source identity stored
                    // separately in the frozen selection.
                    name: reference.callable_name,
                    args: reference.args,
                    resource: None,
                };
                let call_id = format!(
                    "skill-{}",
                    blake3::hash(format!("{input_id}:{reference_index}").as_bytes()).to_hex()
                );
                let args = serde_json::to_value(&input)?;
                self.events
                    .tool_started("load_skill".into(), call_id.clone(), Some(args.clone()))
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
                open_call = Some((call_id.clone(), args.clone()));
                let call = sampling_types::ToolCallResponse {
                    id: call_id.clone(),
                    kind: "function".into(),
                    function: sampling_types::ToolCallFunction::from_json("load_skill", &args),
                };
                let selection = SkillLoadSelection {
                    skill: selected,
                    user_invocation: true,
                    resource_path: None,
                };
                let preflight = USER_SKILL_SELECTION
                    .scope(selection, async {
                        self.prepare_tool_call(call, &mut Vec::new()).await
                    })
                    .await?;
                let prepared = match preflight {
                    ToolPreflight::Dispatch(prepared) => prepared,
                    ToolPreflight::Resolved {
                        loop_result,
                        post_terminal_hook,
                    } => {
                        self.events
                            .tool_completed_durably(
                                &call_id,
                                "not_executed".into(),
                                Some(json!({"dispatched": false})),
                            )
                            .await
                            .map_err(|error| {
                                acp::Error::internal_error().data(error.to_string())
                            })?;
                        open_call = None;
                        if let Some(hook) = post_terminal_hook {
                            self.dispatch_observe_hook(
                                hook.event,
                                hook.cause,
                                hook.payload,
                                hook.prompt_id,
                            )
                            .await
                            .map_err(|error| {
                                acp::Error::internal_error().data(error.to_string())
                            })?;
                        }
                        return Err(acp::Error::invalid_params()
                            .data(format!("Skill load was not admitted: {loop_result:?}")));
                    }
                };
                let authority = ToolDispatchAuthority {
                    bridge: self.tool_bridge_handle(),
                    subagent: self.subagent_capabilities.clone(),
                    mcp_state: self.mcp_state.clone(),
                    runtime_suspend: self.state.lock().await.runtime_suspend.clone(),
                    cwd: self.tool_context.cwd.as_path().to_path_buf(),
                    actor_source: if self.startup_hints.is_subagent {
                        format!("child:{}", self.session_info.id.0)
                    } else {
                        format!("primary:{}", self.turn_behavior.lock().as_id())
                    },
                };
                let result =
                    tool::dispatch::dispatch_tool(&authority, &prepared, &self.session_id_string())
                        .await
                        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
                let output: LoadSkillOutput = match result.output {
                    ToolsToolOutput::Dynamic(output) => serde_json::from_value(output.value)?,
                    _ => {
                        return Err(acp::Error::internal_error()
                            .data("Skill loader returned an invalid output"));
                    }
                };
                bytes = bytes
                    .checked_add(output.prepared_body.len())
                    .filter(|size| *size <= 1_048_576)
                    .ok_or_else(|| {
                        acp::Error::invalid_params()
                            .data("Skill group exceeds the 1 MiB prepared body limit")
                    })?;
                group.items.push(SkillLoadItem {
                    input_id,
                    reference_index,
                    call_id,
                    skill_name: output.skill_name.clone(),
                    source: SkillLoadSource::User,
                    args: output.args.clone(),
                    prepared_body: Arc::from(output.prepared_body.as_str()),
                    prompt_index: Some(prompt_index),
                });
                group.outputs.push(output);
                open_call = None;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            if let Some((call_id, args)) = open_call {
                self.events
                    .tool_completed_durably(
                        &call_id,
                        "failed".into(),
                        Some(json!({"published":false,"error":error.to_string()})),
                    )
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
                self.send_update(
                    acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                        acp::ToolCallId::new(call_id.clone()),
                        acp::ToolCallUpdateFields::new()
                            .status(Some(acp::ToolCallStatus::Failed))
                            .raw_output(Some(
                                json!({"error":"skill_not_loaded","message":error.to_string()}),
                            )),
                    )),
                    None,
                )
                .await;
                let (tool_input, tool_input_truncated) = ::hooks::event::truncate_payload(args);
                self.dispatch_observe_hook(
                    ::hooks::event::HookEventName::PostToolUseFailure,
                    chat_state::HookCause::Tool {
                        call_id: call_id.clone(),
                    },
                    ::hooks::event::HookPayload::PostToolUseFailure {
                        tool_name: "load_skill".into(),
                        tool_use_id: call_id,
                        tool_input,
                        tool_input_truncated,
                        error: error.to_string(),
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                )
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            }
            self.finish_user_skill_loads(&group, false).await?;
            return Err(error);
        }
        Ok(group)
    }

    pub(super) async fn finish_user_skill_loads(
        &self,
        group: &PreparedUserSkills,
        published: bool,
    ) -> Result<(), acp::Error> {
        for (item, output) in group.items.iter().zip(&group.outputs) {
            self.events.tool_completed_durably(&item.call_id,
                if published { "completed" } else { "not_published" }.into(),
                Some(json!({"published": published, "input_id": item.input_id, "reference_index": item.reference_index})))
                .await.map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            let fields = acp::ToolCallUpdateFields::new()
                .status(Some(if published {
                    acp::ToolCallStatus::Completed
                } else {
                    acp::ToolCallStatus::Failed
                }))
                .raw_output(Some(if published {
                    serde_json::to_value(ToolsToolOutput::from(output.clone()))?
                } else {
                    json!({"error":"skill_group_not_published"})
                }));
            self.send_update(
                acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                    acp::ToolCallId::new(item.call_id.clone()),
                    fields,
                )),
                None,
            )
            .await;
            let (tool_input, tool_input_truncated) =
                ::hooks::event::truncate_payload(json!({"name":item.skill_name,"args":item.args}));
            if published {
                let (tool_result, tool_result_truncated) =
                    ::hooks::event::truncate_payload(serde_json::to_value(output)?);
                self.dispatch_observe_hook(
                    ::hooks::event::HookEventName::PostToolUse,
                    chat_state::HookCause::Tool {
                        call_id: item.call_id.clone(),
                    },
                    ::hooks::event::HookPayload::PostToolUse {
                        tool_name: "load_skill".into(),
                        tool_use_id: item.call_id.clone(),
                        tool_input,
                        tool_input_truncated,
                        tool_result,
                        tool_result_truncated,
                        duration_ms: None,
                        is_backgrounded: false,
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                )
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            } else {
                self.dispatch_observe_hook(
                    ::hooks::event::HookEventName::PostToolUseFailure,
                    chat_state::HookCause::Tool {
                        call_id: item.call_id.clone(),
                    },
                    ::hooks::event::HookPayload::PostToolUseFailure {
                        tool_name: "load_skill".into(),
                        tool_use_id: item.call_id.clone(),
                        tool_input,
                        tool_input_truncated,
                        error: "Skill group was not published".into(),
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                )
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            }
        }
        // The next model Step (or the turn terminal on failure) owns closing
        // this Runtime Step through the regular control boundary. Ending it
        // here would lose that boundary's control-admission horizon.
        Ok(())
    }
}
