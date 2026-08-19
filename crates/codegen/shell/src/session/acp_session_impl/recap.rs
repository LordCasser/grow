//! Auxiliary model-call concern for `SessionActor`: side questions, recap
//! generation, and AI-suggest.

use super::*;

use crate::remote::DEFAULT_CONTEXT_WINDOW;
use crate::session::SideQuestionError;
use backon::BackoffBuilder as _;
use sampling_types::SamplingError;

/// Retry policy for the one-shot `/btw` model call: 3 attempts total
/// (1 try + 2 retries), 500ms → 1s jittered backoff. Deliberately short —
/// nothing like the sampler actor's budget — so a fleet-wide capacity event
/// can't multiply side-question traffic into a retry storm.
fn side_question_retry_policy() -> backon::ExponentialBuilder {
    backon::ExponentialBuilder::default()
        .with_max_times(2)
        .with_min_delay(std::time::Duration::from_millis(500))
        .with_max_delay(std::time::Duration::from_secs(1))
        .with_jitter()
}

/// Whether a failed `/btw` attempt is worth retrying: overload only (not
/// every retryable 5xx / stream glitch), minus the shared retry vetoes
/// (`x-should-retry: false`, context length — see
/// [`SamplingError::is_retry_vetoed`], also enforced by the sampler actor's
/// `classify_error`).
fn should_retry_side_question(e: &SamplingError) -> bool {
    e.is_overloaded() && !e.is_retry_vetoed()
}

impl SessionActor {
    /// Handle a /btw side question — single-turn model call using the
    /// parent session's full context.
    ///
    /// Approach:
    /// - Keeps the parent's system prompt (conversation[0]) intact
    /// - Passes the full conversation history (including tool calls/results)
    /// - Includes tool definitions so the model knows capabilities
    /// - Wraps the question in a `<system-reminder>` block in a user message
    /// - Single turn, no tool execution
    ///
    /// The parent Timeline freezes the input range and owns one Sideband spawn;
    /// the independent Sideband ledger owns every request attempt and outcome.
    pub(super) async fn handle_side_question(
        &self,
        question: &str,
    ) -> Result<String, SideQuestionError> {
        let sampling_client = self
            .prepare_chat_completion(false)
            .await
            .map_err(|e| SideQuestionError::PrepareClient(e.to_string()))?;

        // Full conversation snapshot including system prompt, tool calls, and results.
        // Strip reasoning/thinking blocks from assistant items so we don't send
        // `ContentBlock::Thinking` without a top-level `thinking` config. The
        // Anthropic Messages API rejects requests that include thinking blocks in
        // messages but omit the `thinking` parameter.
        let materialized = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await
            .ok_or_else(|| SideQuestionError::Sideband("chat-state actor is unavailable".into()))?;
        let input_ref = materialized.input_ref;
        let mut items: Vec<ConversationItem> =
            chat_state::compaction_utils::strip_reasoning_blocks(materialized.surface);

        // /btw fires mid-turn, so the snapshot may end with an assistant
        // message whose tool_calls have no matching ToolResult yet. The
        // Anthropic Messages API rejects this with "tool_use ids were found
        // without tool_result blocks". Truncate the trailing incomplete
        // assistant+tool_result run.
        while let Some(last) = items.last() {
            match last {
                ConversationItem::Assistant(a) if !a.tool_calls.is_empty() => {
                    items.pop();
                }
                ConversationItem::ToolResult(_) => {
                    items.pop();
                }
                _ => break,
            }
        }

        // Wrap the question in a <system-reminder> user message.
        let tag = self.reminder_wrapper_tag();
        let wrapped_question = format!(
            "<{tag}>This is a side question from the user. \
             You must answer this question directly in a single response.\n\n\
             IMPORTANT CONTEXT:\n\
             - You are a separate, lightweight agent spawned to answer this one question\n\
             - The main agent is NOT interrupted - it continues working independently in the background\n\
             - You share the conversation context but are a completely separate instance\n\
             - Do NOT reference being interrupted or what you were \"previously doing\" - that framing is incorrect\n\n\
             CRITICAL CONSTRAINTS:\n\
             - You have NO tools available - you cannot read files, run commands, search, or take any actions\n\
             - This is a one-off response - there will be no follow-up turns\n\
             - You can ONLY provide information based on what you already know from the conversation context\n\
             - NEVER say things like \"Let me try...\", \"I'll now...\", \"Let me check...\", or promise to take any action\n\
             - If you don't know the answer, say so - do not offer to look it up or investigate\n\n\
             Simply answer the question with the information you have.</{tag}>\n\n\
             {question}"
        );
        items.push(ConversationItem::user(wrapped_question.clone()));

        let tool_definitions = self.prepare_tool_definitions().await;
        let tool_specs: Vec<ToolSpec> = tool_definitions.into_iter().map(ToolSpec::from).collect();

        let model = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();

        // Don't set temperature explicitly — cli-chat-proxy may inject
        // `thinking` config via request_defaults for thinking-enabled models,
        // Anthropic requires temperature == 1 when thinking is enabled.
        // Leaving it None lets the provider defaults apply correctly.
        //
        // Built once; each attempt clones it and stamps a fresh req_id (the
        // per-attempt clone is the cost of the owned-request API — retries
        // are rare, so the success path pays exactly one clone).
        let base_request = ConversationRequest {
            items,
            tools: tool_specs,
            model: Some(model.clone()),
            temperature: None,
            ..Default::default()
        };

        let mut sideband = self
            .begin_sideband(
                chat_state::SidebandPurpose::SideQuestion,
                wrapped_question,
                SidebandSource::Frozen(vec![input_ref]),
                chat_state::SidebandRoute {
                    model: model.clone(),
                    backend: sideband_backend(sampling_client.api_backend()).into(),
                },
                None,
            )
            .await
            .map_err(|error| SideQuestionError::Sideband(error.to_string()))?;

        // conversation_collect is one-shot (no sampler-actor retry); /btw adds
        // its own bounded overload-only retry. Every attempt is durable before
        // the provider request leaves the process.
        let mut backoff = side_question_retry_policy().build();
        let mut feedback = None;
        let result = loop {
            sideband
                .attempt_all_sources(&base_request, feedback.take())
                .await
                .map_err(|error| SideQuestionError::Sideband(error.to_string()))?;
            match sampling_client
                .conversation_collect(base_request.clone())
                .await
            {
                Err(error) if should_retry_side_question(&error) => {
                    let Some(delay) = backoff.next() else {
                        break Err(error);
                    };
                    feedback = Some(error.to_string());
                    tracing::warn!(
                        backoff_ms = delay.as_millis() as u64,
                        error = %error,
                        "side question overload; retrying"
                    );
                    tokio::time::sleep(delay).await;
                }
                settled => break settled,
            }
        };

        match result {
            Ok(response) => {
                let content = response.assistant_text();
                if content.is_empty() {
                    let err = SideQuestionError::EmptyResponse;
                    sideband
                        .fail(chat_state::SidebandOutcome::Failed, err.to_string())
                        .await
                        .map_err(|error| SideQuestionError::Sideband(error.to_string()))?;
                    return Err(err);
                }
                let usage = sideband_usage(&response);
                let finish = sideband_finish(&response);
                sideband
                    .complete(content.clone(), None, usage, finish, Vec::new())
                    .await
                    .map_err(|error| SideQuestionError::Sideband(error.to_string()))?;
                Ok(content)
            }
            Err(e) => {
                let err = SideQuestionError::from(e);
                sideband
                    .fail(chat_state::SidebandOutcome::Failed, err.to_string())
                    .await
                    .map_err(|error| SideQuestionError::Sideband(error.to_string()))?;
                Err(err)
            }
        }
    }

    /// Generate a session recap and broadcast it via
    /// [`SessionUpdate::SessionRecap`](crate::extensions::notification::SessionUpdate::SessionRecap).
    ///
    /// Snapshots the conversation, appends a single recap instruction turn
    /// (reusing the prompt prefix verbatim so the provider cache stays warm),
    /// makes one tool-free model call, and emits the cleaned one-line summary
    /// for display only. It never mutates the conversation.
    ///
    /// Best-effort: a failed or empty generation is logged and dropped — a
    ///
    /// missing recap must never disrupt the session.
    pub(super) async fn handle_recap(&self, auto: bool) {
        use crate::session::helpers::session_recap;

        // Snapshot before the first await so a prompt accepted while we await
        // the conversation reads as bumped-after-capture and cancels this recap.
        let recap_epoch = self.recap_epoch.get();

        let Some(materialized) = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await
        else {
            tracing::warn!("recap: chat-state actor is unavailable");
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        };
        let input_ref = materialized.input_ref;
        let conversation = materialized.surface;
        let main_turns = session_recap::main_turn_count(&conversation);

        let stored = self.last_recap_main_turn.get();
        let last = if stored > main_turns {
            let healed = main_turns.saturating_sub(1);
            self.last_recap_main_turn.set(healed);
            healed
        } else {
            stored
        };

        const RECAP_MIN_IDLE_MS: i64 = 3 * 60 * 1000;
        let last_ms = self
            .last_api_request_at
            .load(std::sync::atomic::Ordering::Relaxed);
        let idle_ms = chrono::Utc::now().timestamp_millis() - last_ms;
        let idle_ok = last_ms != 0 && idle_ms >= RECAP_MIN_IDLE_MS;

        if let Err(reason) = session_recap::recap_gate(main_turns, last, auto, idle_ok) {
            tracing::debug!(auto, main_turns, last, reason, "skipping recap");
            // A manual `/recap` shows a loading spinner; tell the client there
            // is nothing to recap so it can clear it (auto shows none).
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }

        // Serialize recap work: watermark alone cannot exclude concurrent manual
        // re-recaps once last == main_turns (in-flight or finished).
        if self.recap_in_flight.get() {
            tracing::debug!(auto, main_turns, "skipping recap: another recap in flight");
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }
        self.recap_in_flight.set(true);
        // Clear in-flight on every exit. Advance watermark only on success/suppress
        // (not on failure/empty/cancel) so auto can retry later for this turn if needed.
        let clear_in_flight = || self.recap_in_flight.set(false);

        let sampling_client = match self.prepare_chat_completion(false).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "recap: failed to prepare sampling client");
                clear_in_flight();
                // A manual `/recap` shows a loading spinner; clear it on failure.
                if !auto {
                    self.emit_recap_unavailable().await;
                }
                return;
            }
        };

        let tag = self.reminder_wrapper_tag();
        // Strip reasoning only on the Messages backend (it rejects thinking
        // blocks without a `thinking` config). Other backends keep reasoning
        // verbatim so the prefix matches the last turn and the prefix KV
        // cache stays warm. Mirrors compaction's `summary_strips_reasoning`.
        let strip_reasoning =
            sampling_client.api_backend() == crate::sampling::ApiBackend::Messages;

        // Budget off the recap model's context window (today the session model).
        // One read serves both the window and the model.
        let sampling_config = self.chat_state_handle.get_sampling_config().await;
        let context_window = sampling_config
            .as_ref()
            .map(|c| c.context_window.get())
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);
        let items =
            session_recap::budget_recap_items(conversation, tag, strip_reasoning, context_window);

        let model = sampling_config.map(|c| c.model).unwrap_or_default();

        // Leave BOTH temperature and max_output_tokens unset: the cli-chat-proxy
        // layer may inject a `thinking` budget for thinking-enabled models
        // (which also forces temperature == 1), and a small max_output_tokens
        // below that budget makes the call error or return empty — silently
        // dropping the recap. The recap instruction keeps the body to
        // ~25–40 words, and `clean_recap_text` caps it at a generous
        // RECAP_MAX_CHARS safety net, so an explicit token cap isn't needed.
        // Main-turn tool specs: tools serialize into the cached token prefix.
        let tool_defs = self.prepare_tool_definitions().await;
        let tools = self.turn_base_tool_specs(&tool_defs);
        let request = ConversationRequest {
            items,
            tools,
            model: Some(model.clone()),
            temperature: None,
            prompt_cache_key: Some(self.session_info.id.to_string()),
            ..Default::default()
        };

        let mut sideband = match self
            .begin_sideband(
                chat_state::SidebandPurpose::SessionRecap,
                session_recap::recap_instruction(tag),
                SidebandSource::Frozen(vec![input_ref]),
                chat_state::SidebandRoute {
                    model: model.clone(),
                    backend: sideband_backend(sampling_client.api_backend()).into(),
                },
                None,
            )
            .await
        {
            Ok(sideband) => sideband,
            Err(error) => {
                tracing::warn!(%error, "recap: failed to start Sideband");
                clear_in_flight();
                if !auto {
                    self.emit_recap_unavailable().await;
                }
                return;
            }
        };
        if let Err(error) = sideband.attempt_all_sources(&request, None).await {
            tracing::warn!(%error, "recap: failed to commit Sideband attempt");
            clear_in_flight();
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }

        let response = match sampling_client.conversation_collect(request).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "recap: model call failed");
                if let Err(record_error) = sideband
                    .fail(chat_state::SidebandOutcome::Failed, e.to_string())
                    .await
                {
                    tracing::warn!(%record_error, "recap: failed to commit Sideband failure");
                }
                clear_in_flight();
                // A manual `/recap` shows a loading spinner; clear it on failure.
                if !auto {
                    self.emit_recap_unavailable().await;
                }
                return;
            }
        };

        let raw_response = response.assistant_text();
        let summary = session_recap::clean_recap_text(&raw_response);
        if summary.is_empty() {
            tracing::debug!("recap: model returned empty summary");
            if let Err(record_error) = sideband
                .fail(
                    chat_state::SidebandOutcome::Failed,
                    "empty summary after clean_recap_text",
                )
                .await
            {
                tracing::warn!(%record_error, "recap: failed to commit empty Sideband result");
            }
            clear_in_flight();
            // A manual `/recap` shows a loading spinner; clear it when empty.
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }

        let usage = sideband_usage(&response);
        let finish = sideband_finish(&response);
        if let Err(error) = sideband
            .complete(
                raw_response.clone(),
                Some(serde_json::json!({ "summary": summary.clone() })),
                usage,
                finish,
                Vec::new(),
            )
            .await
        {
            tracing::warn!(%error, "recap: failed to commit Sideband result");
            clear_in_flight();
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }

        // New prompt while generating: keep the completed Sideband, skip
        // display, and leave the watermark unchanged.
        // Applies to manual `/recap` too: spinner-less clients (e.g. Grow
        // an embedding client) would otherwise append the late recap mid-turn.
        if self.recap_was_cancelled(recap_epoch) {
            tracing::info!(
                auto,
                recap_epoch,
                current_epoch = self.recap_epoch.get(),
                "session recap cancelled (new prompt while generating; not shown)"
            );
            self.drop_recap_after_cancel(auto).await;
            return;
        }

        // Auto long-tail: preserve the completed Sideband but do not show it.
        // Manual always shows.
        if auto && session_recap::should_suppress_auto_recap_display(&raw_response, &summary) {
            tracing::info!(
                raw_bytes = raw_response.len(),
                summary_bytes = summary.len(),
                "session recap suppressed (auto long-tail; Sideband saved, not shown)"
            );
            // Commit watermark only if still live (no await between check and mark).
            let _ = self.try_commit_recap(recap_epoch, main_turns);
            return;
        }

        tracing::info!(auto, chars = summary.len(), "session recap generated");
        // Final cancel check immediately before mark+emit (no await between).
        if !self.try_commit_recap(recap_epoch, main_turns) {
            if !auto {
                self.emit_recap_unavailable().await;
            }
            return;
        }
        self.send_grow_notification(
            crate::extensions::notification::SessionUpdate::SessionRecap { summary, auto },
        )
        .await;
    }

    /// Invalidate in-flight recap (real user prompt at queue time / turn start).
    pub(crate) fn cancel_pending_recap_for_new_prompt(&self) {
        self.recap_epoch.set(self.recap_epoch.get().wrapping_add(1));
    }

    /// Whether `epoch` is stale because a newer prompt started.
    pub(crate) fn recap_was_cancelled(&self, epoch: u64) -> bool {
        self.recap_epoch.get() != epoch
    }

    /// If still live, advance watermark and clear in-flight; else clear only.
    /// Returns whether the recap may emit (or count as done for suppress).
    pub(crate) fn try_commit_recap(&self, recap_epoch: u64, main_turns: usize) -> bool {
        if self.recap_was_cancelled(recap_epoch) {
            self.recap_in_flight.set(false);
            false
        } else {
            self.last_recap_main_turn.set(main_turns);
            self.recap_in_flight.set(false);
            true
        }
    }

    /// Cancel-branch cleanup after generation: clear in-flight; manual clients
    /// get `SessionRecapUnavailable` so their spinner can clear.
    pub(crate) async fn drop_recap_after_cancel(&self, auto: bool) {
        self.recap_in_flight.set(false);
        if !auto {
            self.emit_recap_unavailable().await;
        }
    }

    /// Tell the live client that a manual `/recap` produced no recap, so it can
    ///
    /// Only the manual path shows a spinner, so callers gate this on `!auto`.
    async fn emit_recap_unavailable(&self) {
        self.send_grow_notification(
            crate::extensions::notification::SessionUpdate::SessionRecapUnavailable,
        )
        .await;
    }

    /// Handle an AI-powered shell command suggestion request.
    /// Builds a minimal prompt from the prefix and CWD, calls the sampler
    ///
    /// and returns the suggestion. Sampling preferences come from the selected
    /// model configuration or, when unset there, from the upstream service.
    pub(super) async fn handle_ai_suggest(
        &self,
        prefix: &str,
        cwd: &str,
        model_override: Option<&str>,
    ) -> Option<String> {
        let sampling_client = self.prepare_chat_completion(false).await.ok()?;

        let system = "You are a shell command autocomplete engine. \
            Given a partial command, output ONLY the completed command. \
            No explanation, no markdown, no quotes. Just the raw command.";

        let user_msg = format!("CWD: {cwd}\nPartial command: {prefix}");
        let sideband_prompt = format!("{system}\n\n{user_msg}");

        let items = vec![
            ConversationItem::system(system.to_owned()),
            ConversationItem::user(user_msg),
        ];

        let model = match model_override {
            Some(m) => m.to_owned(),
            None => "grow-build".to_owned(),
        };

        let request = ConversationRequest {
            items,
            tools: vec![],
            model: Some(model),
            ..Default::default()
        };

        let request_id = sampler::RequestId::random();
        let idle_timeout = std::time::Duration::from_secs(5);
        let mut sideband = self
            .begin_sideband(
                chat_state::SidebandPurpose::PromptSuggestion,
                sideband_prompt,
                SidebandSource::None,
                chat_state::SidebandRoute {
                    model: request.model.clone().unwrap_or_default(),
                    backend: sideband_backend(sampling_client.api_backend()).into(),
                },
                None,
            )
            .await
            .ok()?;
        sideband.attempt_all_sources(&request, None).await.ok()?;

        let result = match sampling_client.api_backend() {
            crate::sampling::ApiBackend::ChatCompletions => {
                let (raw, meta) = match sampling_client.conversation_stream(request).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = sideband
                            .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                            .await;
                        return None;
                    }
                };
                let events = sampler::stream_chat_completions(raw, meta, request_id, idle_timeout);
                sampler::collect_response(events).await
            }
            crate::sampling::ApiBackend::Responses => {
                let (raw, meta, doom_loop) =
                    match sampling_client.conversation_stream_responses(request).await {
                        Ok(stream) => stream,
                        Err(error) => {
                            let _ = sideband
                                .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                                .await;
                            return None;
                        }
                    };
                let events =
                    sampler::stream_responses(raw, meta, request_id, idle_timeout, doom_loop);
                sampler::collect_response(events).await
            }
            crate::sampling::ApiBackend::Messages => {
                let (raw, meta) = match sampling_client.conversation_stream_messages(request).await
                {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = sideband
                            .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                            .await;
                        return None;
                    }
                };
                let events = sampler::stream_messages(raw, meta, request_id, idle_timeout);
                sampler::collect_response(events).await
            }
        };

        match result {
            Ok((response, _metrics)) => {
                let text = response.assistant_text();
                if text.is_empty() {
                    let _ = sideband
                        .fail(
                            chat_state::SidebandOutcome::Failed,
                            "AI suggest returned an empty command",
                        )
                        .await;
                    None
                } else {
                    let usage = sideband_usage(&response);
                    let finish = sideband_finish(&response);
                    sideband
                        .complete(text.clone(), None, usage, finish, Vec::new())
                        .await
                        .ok()?;
                    Some(text)
                }
            }
            Err(e) => {
                tracing::debug!(error = %e.message, "AI suggest inference failed");
                let _ = sideband
                    .fail(chat_state::SidebandOutcome::Failed, e.message)
                    .await;
                None
            }
        }
    }

    /// Predict the user's likely next prompt for tab-autocomplete ghost text.
    /// Fired by the client after a turn completes. Builds a compact text-only
    /// transcript of the recent conversation (see
    /// [`prompt_suggest::build_transcript`]) and makes one tool-free model
    /// call. The model is resolved by
    /// [`prompt_suggest::effective_suggest_model`]: env
    /// (`GROW_PROMPT_SUGGESTIONS_MODEL`) > `[models] prompt_suggestion`
    /// (config.toml) > remote `prompt_suggestion_model` (remote settings) >
    /// client hint. Every tier except env is catalog-guarded against this
    /// shell's own model catalog. Without an explicit model the request is
    /// skipped entirely. The session model is never used:
    /// a per-turn background call must stay on the small model.
    /// Temperature, max_output_tokens, and
    /// reasoning_effort are left unset — mirrors [`Self::handle_recap`]: the
    /// proxy may inject provider defaults, a small token cap silently empties
    /// a reasoning model's response, and some models (e.g. `grow-build`)
    /// reject an explicit `reasoningEffort` with a 400. Output is filtered
    /// through [`prompt_suggest::sanitize_suggestion`]; any failure returns
    /// through [`prompt_suggest::sanitize_suggestion`]; any failure returns
    /// `None`.
    pub(super) async fn handle_suggest_prompt(
        &self,
        model_override: Option<&str>,
    ) -> Option<String> {
        use crate::session::helpers::prompt_suggest;

        let pin = self.models_manager.prompt_suggest_model_pin();
        let Some(model) = prompt_suggest::effective_suggest_model(&pin, model_override, |m| {
            self.models_manager.model_in_catalog(m)
        }) else {
            tracing::debug!(
                pin = ?pin,
                client_hint = ?model_override,
                "prompt suggest: effective model not in catalog; skipping request"
            );
            return None;
        };

        let materialized = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await?;
        let input_ref = materialized.input_ref;
        let conversation = materialized.surface;
        let Some(transcript) = prompt_suggest::build_transcript(&conversation) else {
            tracing::debug!(
                items = conversation.len(),
                "prompt suggest: no usable transcript"
            );
            return None;
        };

        let sampling_client = match self.prepare_chat_completion(false).await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "prompt suggest: sampling client unavailable");
                return None;
            }
        };

        tracing::debug!(
            model = %model,
            transcript_len = transcript.len(),
            "prompt suggest: requesting"
        );

        let cwd = self
            .tool_context
            .cwd
            .as_path()
            .to_string_lossy()
            .into_owned();
        let user_prompt = prompt_suggest::suggest_prompt_user_message(&transcript, &cwd);
        let items = vec![
            ConversationItem::system(prompt_suggest::SUGGEST_PROMPT_SYSTEM.to_owned()),
            ConversationItem::user(user_prompt),
        ];

        let request = ConversationRequest {
            items,
            tools: vec![],
            model: Some(model),
            temperature: None,
            ..Default::default()
        };

        let mut sideband = self
            .begin_sideband(
                chat_state::SidebandPurpose::PromptSuggestion,
                format!("{}\n\nCWD: {cwd}", prompt_suggest::SUGGEST_PROMPT_SYSTEM),
                SidebandSource::Frozen(vec![input_ref]),
                chat_state::SidebandRoute {
                    model: request.model.clone().unwrap_or_default(),
                    backend: sideband_backend(sampling_client.api_backend()).into(),
                },
                None,
            )
            .await
            .ok()?;
        sideband.attempt_all_sources(&request, None).await.ok()?;

        let response = match sampling_client.conversation_collect(request).await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "prompt suggest inference failed");
                let _ = sideband
                    .fail(chat_state::SidebandOutcome::Failed, e.to_string())
                    .await;
                return None;
            }
        };

        let raw = response.assistant_text();
        let mut suggestion = prompt_suggest::sanitize_suggestion(&raw);
        // Deterministic anti-repeat backstop: never ghost a multi-word
        // prompt the user already sent (the prompt asks the model not to,
        // but a filter guarantees it).
        if let Some(s) = &suggestion
            && prompt_suggest::is_repeat_of_user_message(s, &conversation)
        {
            tracing::debug!("prompt suggest: rejected repeat of a past user prompt");
            suggestion = None;
        }
        if let Some(accepted) = &suggestion {
            let usage = sideband_usage(&response);
            let finish = sideband_finish(&response);
            sideband
                .complete(
                    raw.clone(),
                    Some(serde_json::json!({ "suggestion": accepted })),
                    usage,
                    finish,
                    Vec::new(),
                )
                .await
                .ok()?;
        } else {
            sideband
                .fail(
                    chat_state::SidebandOutcome::Failed,
                    "prompt suggestion failed local output validation",
                )
                .await
                .ok()?;
        }
        tracing::debug!(
            raw_preview = %tools::util::truncate_str(raw.trim(), 60),
            accepted = suggestion.is_some(),
            "prompt suggest: response"
        );
        suggestion
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, message: &str, should_retry: Option<bool>) -> SamplingError {
        SamplingError::Api {
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            message: message.into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry,
        }
    }

    #[test]
    fn side_question_retries_overload_only() {
        // Typed stream overload, its proxy-wrapped 500 shape, and 529 retry.
        assert!(should_retry_side_question(
            &SamplingError::from_stream_error("overloaded_error", "Overloaded")
        ));
        assert!(should_retry_side_question(&api(
            500,
            "stream error (overloaded_error): Overloaded",
            None
        )));
        assert!(should_retry_side_question(&api(529, "capacity", None)));

        // Server veto (`x-should-retry: false`) wins over overload.
        assert!(!should_retry_side_question(&api(
            529,
            "capacity",
            Some(false)
        )));
        // Deterministic context-length failures never retry, even on 529.
        assert!(!should_retry_side_question(&api(
            529,
            "invalid_request_error: prompt is too long: 300000 tokens > 200000 maximum",
            None
        )));
        // Rate limit and generic 5xx are not overload — no /btw retry.
        assert!(!should_retry_side_question(&api(429, "slow down", None)));
        assert!(!should_retry_side_question(&api(
            503,
            "upstream connect timeout",
            None
        )));
    }

    /// The wired policy: 3 attempts total, backoff within the configured
    /// bounds (500ms + 1s base, jitter adds up to the current delay), and a
    /// fresh request id stamped per attempt.
    #[tokio::test(start_paused = true)]
    async fn side_question_retry_wiring_caps_attempts_and_bounds_backoff() {
        use backon::Retryable as _;

        let calls = std::cell::Cell::new(0u32);
        let start = tokio::time::Instant::now();
        let result: Result<(), SamplingError> = (|| async {
            calls.set(calls.get() + 1);
            Err(SamplingError::from_stream_error(
                "overloaded_error",
                "Overloaded",
            ))
        })
        .retry(side_question_retry_policy())
        .when(should_retry_side_question)
        .await;

        assert!(result.is_err());
        assert_eq!(calls.get(), 3, "1 try + 2 retries");
        // Base delays 500ms + 1s; jitter adds (0, delay) per sleep.
        let elapsed = start.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(1_500),
            "elapsed {elapsed:?} below minimum backoff"
        );
        assert!(
            elapsed <= std::time::Duration::from_millis(3_100),
            "elapsed {elapsed:?} above maximum backoff"
        );
    }
}
