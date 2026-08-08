//! Turn-completion concern for `SessionActor`: completion handling
//! and turn-error classification.

use super::*;

impl SessionActor {
    /// Emit a cosmetic `Plan` update at turn end to clear stale spinners.
    ///
    /// When the model produces its final text response without a cleanup
    /// `todo_write` call, any remaining `in_progress` items leave stale
    /// spinners in the UI.
    ///
    /// This method **does not mutate** the underlying `TodoState` — the
    /// persisted resource state and the model's view of the todo list
    /// are unchanged.  It only emits a transient (non-persisted) `Plan`
    /// notification where `in_progress` entries are mapped to `completed`
    /// for display.
    ///
    /// Uses the canonical `plan_entry_from_todo_item` helper to preserve
    /// cancelled metadata, priorities, and other semantics.
    ///
    /// No-op if no `in_progress` items exist.
    pub(super) async fn emit_turn_end_plan_cleanup(&self) {
        use crate::tools::todo::{TodoState, TodoStatus, plan_entry_from_todo_item};
        use tools::types::resources::State;

        // Read the current TodoState (no mutation).
        let (entries, stale_count) = {
            let res = self
                .agent
                .borrow()
                .tool_bridge()
                .read_resource::<State<TodoState>>()
                .await;
            let Some(state) = res else {
                return; // No todo state at all.
            };

            let stale_count = state
                .0
                .todo_items()
                .filter(|t| t.status == TodoStatus::InProgress)
                .count();
            if stale_count == 0 {
                return;
            }

            // Build plan entries with in_progress → completed for display.
            // Uses the canonical `plan_entry_from_todo_item` helper to
            // preserve cancelled metadata, priority, and other semantics.
            let entries: Vec<_> = state
                .0
                .todo_items()
                .map(|item| {
                    let mut entry = plan_entry_from_todo_item(item.clone());
                    if item.status == TodoStatus::InProgress {
                        entry.status = acp::PlanEntryStatus::Completed;
                    }
                    entry
                })
                .collect();

            (entries, stale_count)
        };

        tracing::info!(
            stale_count,
            "emitting transient turn-end Plan cleanup — in_progress shown as completed"
        );

        // Use transient notification — this is a cosmetic UI update that
        // must NOT be persisted or replayed on session reload.  The real
        // TodoState in Resources is the source of truth.
        let notification = acp::SessionNotification::new(
            self.session_info.id.clone(),
            acp::SessionUpdate::Plan(acp::Plan::new(entries)),
        );
        self.emit_transient_notification(notification);
    }

    /// Emit `grow/git_head_changed` after an edit/shell command that may have
    /// moved HEAD (e.g. `git checkout`), so clients update their status bar
    /// immediately rather than waiting for the debounced fs-watch refresh.
    pub(super) async fn maybe_notify_git_branch(&self) {
        if !self.git_head_enabled {
            return;
        }
        let cwd = self.tool_context.cwd.as_path();

        // `get_worktree_info` doubles as the "in a git repo?" probe (None when not).
        let (worktree_info, branch) = tokio::join!(
            workspace::session::git::get_worktree_info(cwd),
            workspace::session::git::get_branch(cwd),
        );
        let Some((is_worktree, main_repo)) = worktree_info else {
            return;
        };

        let dedup_key = git_head_dedup_key(branch.as_deref(), is_worktree, main_repo.as_deref());
        {
            let mut last = self.last_reported_branch.lock();
            if last.as_deref() == Some(&dedup_key) {
                return;
            }
            *last = Some(dedup_key);
        }

        let params = workspace::session::git::GitHeadChanged {
            session_id: self.session_info.id.0.to_string(),
            branch,
            is_worktree,
            main_repo,
        };
        if let Ok(raw) = serde_json::value::to_raw_value(&params) {
            let notification = acp::ExtNotification::new("grow/git_head_changed", raw.into());
            self.notifications
                .gateway
                .forward_fire_and_forget(notification);
        }
    }

    /// Live subagents and sticky usage-not-applied. `None` if the query failed.
    pub(super) async fn outstanding_reply_for_prompt(
        &self,
        prompt_id: &str,
    ) -> Option<tools::implementations::grow_build::task::types::SubagentOutstandingReply> {
        let Some(tx) = &self.tool_context.subagent_event_tx else {
            return Some(Default::default());
        };
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentOutstandingRequest,
        };
        let (respond_to, rx) = tokio::sync::oneshot::channel();
        if tx
            .send(SubagentEvent::Outstanding(SubagentOutstandingRequest {
                parent_session_id: self.session_id_string(),
                prompt_id: prompt_id.to_string(),
                respond_to,
            }))
            .is_err()
        {
            return None;
        }
        rx.await.ok()
    }

    /// Report-level incomplete (error-path attach, tests). Same OR as
    /// [`super::turn::UsageDrainOutcome::report_incomplete`].
    pub(super) fn usage_incomplete_from_reply(
        reply: Option<&tools::implementations::grow_build::task::types::SubagentOutstandingReply>,
    ) -> bool {
        super::turn::UsageDrainOutcome::from_outstanding_reply(reply).report_incomplete()
    }

    pub(super) fn clear_subagent_usage_not_applied(&self, prompt_id: &str) {
        let Some(tx) = &self.tool_context.subagent_event_tx else {
            return;
        };
        use tools::implementations::grow_build::task::types::{
            SubagentClearUsageNotAppliedRequest, SubagentEvent,
        };
        let _ = tx.send(SubagentEvent::ClearUsageNotApplied(
            SubagentClearUsageNotAppliedRequest {
                parent_session_id: self.session_id_string(),
                prompt_id: prompt_id.to_string(),
            },
        ));
    }

    pub(super) async fn handle_completion(&self, prompt_id: String, result: PromptTurnResult) {
        // Settle the exact foreground owner first. An internal Goal turn is
        // intentionally absent from `pending_inputs`, so FIFO membership can
        // never be used as the ownership test.
        let (settled_input, broadcast_queue, goal_finalization) = {
            let mut state = self.state.lock().await;
            if state.running_prompt_id() != Some(prompt_id.as_str()) {
                tracing::warn!("Received completion for unknown prompt: {prompt_id}");
                ::diagnostics::unified_log::warn(
                    "shell.turn.stale_completion_dropped",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({
                        "prompt_id": prompt_id,
                        "running_prompt_id": state.running_prompt_id(),
                    })),
                );
                return;
            }
            let task = state
                .foreground
                .take_regular()
                .expect("running prompt id implies a regular foreground task");
            let goal_finalization = matches!(
                task.origin,
                crate::session::PromptOrigin::GoalFinalization { .. }
            );
            let input = state
                .pending_inputs
                .front()
                .is_some_and(|input| input.prompt_id == prompt_id)
                .then(|| state.pending_inputs.pop_front())
                .flatten();
            let broadcast = input.as_ref().is_some_and(|item| item.queue_meta.is_some());
            (input, broadcast, goal_finalization)
        };

        if let Some(input) = settled_input {
            let _ = input.respond_to.send(result.clone());
        }
        {
            let mut current_prompt_id = self
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned");
            if current_prompt_id.as_deref() == Some(prompt_id.as_str()) {
                *current_prompt_id = None;
            }
        }
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentPromptIdResource(
                    String::new(),
                ),
            )
            .await;
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentSubagentOwnerResource::default(),
            )
            .await;
        self.set_goal_loop_active_resource(false).await;
        self.flush_pending_system_reminders().await;

        let emit_terminal = !matches!(
            result,
            Ok(PromptTurnOk {
                completion_kind: PromptCompletionKind::RemovedFromQueue,
                ..
            })
        );
        if !emit_terminal {
            tracing::warn!("Received completion for unknown prompt: {prompt_id}");
        } else {
            let mapped = result
                .as_ref()
                .map(|ok| ok.stop_reason)
                .map_err(Clone::clone);
            let usage = match &result {
                Ok(ok) => ok.usage.clone(),
                Err(err) => {
                    if let Some(u) = crate::sampling::error::prompt_usage_from_error(err) {
                        Some(u)
                    } else {
                        self.error_path_usage_fallback(&prompt_id).await
                    }
                }
            };
            // Surface the cancel trigger on the terminal `_meta` as `cancelTrigger`.
            let cancel_trigger = result
                .as_ref()
                .ok()
                .and_then(|ok| match &ok.completion_kind {
                    PromptCompletionKind::Cancelled {
                        context: Some(ctx), ..
                    } => ctx.trigger.as_deref(),
                    _ => None,
                });
            self.emit_turn_completed(prompt_id, &mapped, usage, cancel_trigger)
                .await;
            if goal_finalization && Self::goal_finalization_terminal_succeeded(&result) {
                self.finalize_goal_finalization_turn().await;
            }
        }
        // The terminal is durable before clients observe either a new running
        // owner or an idle queue snapshot.
        if broadcast_queue {
            let state = self.state.lock().await;
            self.broadcast_queue_changed(&state);
        }
    }

    /// Emit the durable, replayable `TurnCompleted` terminal — the single
    /// chokepoint shared by the completion (`handle_completion`) and cancel
    /// (`cancel_running_task`) sites. Derives `(stop_reason, agent_result)`
    /// from the same source as PromptResponse (`prompt_complete_fields`), then
    /// persists + forwards via `send_grow_notification`.
    ///
    /// `cancel_trigger` (when `Some`) rides the `_meta` as `cancelTrigger`.
    pub(super) async fn emit_turn_completed(
        &self,
        prompt_id: String,
        mapped: &std::result::Result<acp::StopReason, acp::Error>,
        usage: Option<crate::extensions::notification::PromptUsage>,
        cancel_trigger: Option<&str>,
    ) {
        let (stop_reason, agent_result) = crate::sampling::error::prompt_complete_fields(mapped);
        self.state.lock().await.record_recent_terminal(
            crate::session::prompt_queue::RecentPromptTerminal {
                prompt_id: prompt_id.clone(),
                stop_reason: stop_reason.as_str().unwrap_or("error").to_string(),
                agent_result: agent_result.as_str().map(str::to_string),
            },
        );
        let extra_meta = cancel_trigger.map(|t| {
            [("cancelTrigger".to_string(), serde_json::json!(t))]
                .into_iter()
                .collect()
        });
        self.send_grow_notification_with_extra_meta(
            crate::session::turn_completion::build_turn_completed(
                prompt_id,
                stop_reason,
                agent_result,
                usage,
            ),
            extra_meta,
        )
        .await;
    }

    /// Diagnostic error category; delegates to `stop_failure_error_type` so the
    /// two classifications cannot drift.
    pub(super) fn classify_turn_error(err: &acp::Error) -> String {
        use ::hooks::event::StopFailureKind as K;
        match Self::stop_failure_error_type(err) {
            K::RateLimit => "rate_limit",
            K::AuthenticationFailed => "auth",
            K::InvalidRequest => "invalid_request",
            K::ServerError => "internal",
            K::MaxOutputTokens => "max_tokens",
            K::Unknown => "unknown",
        }
        .to_string()
    }

    /// The `StopFailure` hook input's classified `error`. Structured markers win
    /// over the JSON-RPC code because they are more specific; anything the
    /// runtime cannot distinguish stays `Unknown`.
    pub(super) fn stop_failure_error_type(err: &acp::Error) -> ::hooks::event::StopFailureKind {
        use ::hooks::event::StopFailureKind as K;
        if crate::sampling::error::stop_reason_for_turn_error(err) == "MaxTokens" {
            return K::MaxOutputTokens;
        }
        // The data-carried HTTP status discriminates over the JSON-RPC code. 403
        // is content-safety, not auth: it folds into `invalid_request` on the turn
        // path (carries `http_status: 403`) and `server_error` on the setup path
        // (no status, so `-32603` below).
        match crate::sampling::error::http_status_from_error(err) {
            Some(401) => return K::AuthenticationFailed,
            Some(429) | Some(503) | Some(529) => return K::RateLimit,
            Some(s) if (400..500).contains(&s) => return K::InvalidRequest,
            Some(s) if s >= 500 => return K::ServerError,
            _ => {}
        }
        match i32::from(err.code) {
            crate::sampling::error::RATE_LIMITED_ERROR_CODE => K::RateLimit,
            -32000 => K::AuthenticationFailed,
            -32002 | -32600 | -32602 => K::InvalidRequest,
            -32603 => K::ServerError,
            _ => K::Unknown,
        }
    }

    /// Whether a turn error is transient infra worth a goal retry. Keys on the
    /// JSON-RPC code only (unlike `stop_failure_error_type`), so `-32603` counts
    /// as infra.
    pub(super) fn is_infra_turn_error(err: &acp::Error) -> bool {
        matches!(
            i32::from(err.code),
            crate::sampling::error::RATE_LIMITED_ERROR_CODE | -32000 | -32603
        )
    }

    /// A Goal completion receipt requires a real successful final report.
    /// Refusal, cancellation, max-turn termination, and stationarity are all
    /// terminal events, but none proves that the summarizing turn delivered
    /// the report the verifier authorized.
    pub(super) fn goal_finalization_terminal_succeeded(result: &PromptTurnResult) -> bool {
        result.as_ref().ok().is_some_and(|ok| {
            ok.stop_reason == acp::StopReason::EndTurn
                && matches!(ok.completion_kind, PromptCompletionKind::Completed)
        })
    }

    /// `(turn_succeeded, suppress_goal_continuation, infra_pause_message)`.
    /// StationarityEnded is success but suppresses the next idle continuation.
    /// `infra_pause_message` is extracted before `handle_completion` consumes `result`.
    pub(super) fn post_turn_goal_degradation_plan(
        result: &PromptTurnResult,
        origin: Option<&crate::session::PromptOrigin>,
    ) -> (bool, bool, Option<String>) {
        let suppress_goal_continuation = result.as_ref().ok().is_some_and(|ok| {
            matches!(
                ok.completion_kind,
                crate::session::commands::PromptCompletionKind::StationarityEnded
            )
        });
        let turn_cancelled = result.as_ref().ok().is_some_and(|ok| {
            matches!(
                ok.completion_kind,
                crate::session::commands::PromptCompletionKind::Cancelled { .. }
                    | crate::session::commands::PromptCompletionKind::MaxTurnsReached { .. }
            )
        });
        let turn_succeeded = result
            .as_ref()
            .ok()
            .is_some_and(|ok| !turn_cancelled && ok.stop_reason != acp::StopReason::Refusal);
        let infra_pause_message = result
            .as_ref()
            .err()
            .filter(|_| origin.is_some_and(crate::session::PromptOrigin::is_goal_internal))
            .filter(|err| Self::is_infra_turn_error(err))
            .map(Self::format_turn_error_message);
        (
            turn_succeeded,
            suppress_goal_continuation,
            infra_pause_message,
        )
    }

    pub(super) async fn apply_infra_pause_after_turn_err(&self, message: String) -> bool {
        let slash_detail = match message.strip_prefix("Turn failed: ") {
            Some(rest) => rest.to_owned(),
            None => message.clone(),
        };
        let paused = self
            .auto_pause_goal_if_active_with_message(
                crate::session::goal_tracker::GoalPauseReason::Infra,
                message,
            )
            .await;
        if paused {
            self.send_slash_command_output(&format!(
                "Goal paused due to turn error: {slash_detail}. Use /goal resume to retry."
            ))
            .await;
        }
        paused
    }

    /// Extract the best human-readable detail from an infra turn error.
    pub(super) fn turn_error_detail(err: &acp::Error) -> Option<String> {
        err.data
            .as_ref()
            .and_then(crate::sampling::error::error_detail_from_data)
            .or_else(|| {
                if !err.message.is_empty() {
                    Some(err.message.clone())
                } else {
                    None
                }
            })
    }

    pub(super) fn format_turn_error_message(err: &acp::Error) -> String {
        if let Some(detail) = Self::turn_error_detail(err) {
            format!("Turn failed: {detail}")
        } else {
            format!("Turn failed: {}", Self::classify_turn_error(err))
        }
    }

    pub(super) fn classify_install_error(
        err: &agent::plugins::install_registry::InstallError,
    ) -> String {
        crate::plugin::classify_install_error(err)
    }
}
