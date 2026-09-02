//! Durable sideband lifecycle shared by every auxiliary model call.

use sampling_types::{ConversationRequest, ConversationResponse};

use crate::session::SessionActor;
use crate::session::notifications::NotificationSender;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SidebandSource {
    None,
    Frozen(Vec<chat_state::TimelineRangeRef>),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SidebandRunError {
    #[error("sideband lifecycle is invalid: {0}")]
    Invalid(#[from] chat_state::SidebandError),
    #[error("sideband parent Timeline commit failed: {0}")]
    Parent(#[from] chat_state::TimelineWriteError),
    #[error("sideband persistence failed: {0}")]
    Persistence(String),
    #[error("sideband persistence was cancelled")]
    Cancelled,
    #[error("an interrupted sideband append was recovered; the new operation was not applied")]
    InterruptedAppendRecovered,
    #[error("sideband provider admission was rejected: {0}")]
    Admission(String),
}

pub(crate) struct SidebandRun {
    background: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    timeline: chat_state::SidebandTimeline,
    persistence: NotificationSender,
    cancellation: tokio_util::sync::CancellationToken,
    repair_cancellation: tokio_util::sync::CancellationToken,
    fail_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    goal_usage_window: super::goal_support::GoalUsageWindow,
    usage_owner_id: String,
    usage_epoch: u64,
    expected_goal_id: Option<String>,
    admitted_attempt_id: Option<String>,
    pending: Option<chat_state::SidebandEvent>,
    persistence_poison: Option<String>,
    activity: Option<super::tasks_cancel::SessionActivityPermit>,
}

impl SessionActor {
    /// Atomically close Sideband writer admission before the final activity
    /// drain. The same gate guards activity acquisition in `begin_sideband`,
    /// so the final idle observation cannot race a late nested writer.
    pub(super) async fn fail_stop_sideband_admission(&self) {
        let _admission = self.sideband_admission_gate.lock().await;
        self.sideband_fail_stop
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Commit a spawn fact and the sideband request before the caller may emit
    /// the auxiliary provider request. `Frozen` ranges were materialized in one
    /// parent-actor query, so later parent appends cannot expand them.
    pub(crate) async fn begin_sideband(
        &self,
        purpose: chat_state::SidebandPurpose,
        prompt: String,
        source: SidebandSource,
        budget_policy: chat_state::SidebandBudgetPolicy,
        route: chat_state::SidebandRoute,
        output_schema: Option<serde_json::Value>,
    ) -> Result<SidebandRun, SidebandRunError> {
        self.begin_sideband_in_epoch(
            purpose,
            prompt,
            source,
            budget_policy,
            route,
            output_schema,
            false,
        )
        .await
    }

    /// SessionEnd is the only lifecycle phase allowed to create auxiliary
    /// model work after ordinary activity admission closes. Keeping this as a
    /// separate entry point prevents a late regular caller from inheriting
    /// finalizer authority from ambient Session state.
    pub(super) async fn begin_finalizer_sideband(
        &self,
        purpose: chat_state::SidebandPurpose,
        prompt: String,
        source: SidebandSource,
        budget_policy: chat_state::SidebandBudgetPolicy,
        route: chat_state::SidebandRoute,
        output_schema: Option<serde_json::Value>,
    ) -> Result<SidebandRun, SidebandRunError> {
        self.begin_sideband_in_epoch(
            purpose,
            prompt,
            source,
            budget_policy,
            route,
            output_schema,
            true,
        )
        .await
    }

    async fn begin_sideband_in_epoch(
        &self,
        purpose: chat_state::SidebandPurpose,
        prompt: String,
        source: SidebandSource,
        budget_policy: chat_state::SidebandBudgetPolicy,
        route: chat_state::SidebandRoute,
        output_schema: Option<serde_json::Value>,
        finalizer: bool,
    ) -> Result<SidebandRun, SidebandRunError> {
        let activity = {
            let _admission = self.sideband_admission_gate.lock().await;
            if self
                .sideband_fail_stop
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Err(SidebandRunError::Admission(
                    "session Sideband writer epoch is closed".to_string(),
                ));
            }
            if finalizer {
                self.session_activities.start_nested("sideband_finalizer")
            } else {
                self.session_activities
                    .try_start("sideband")
                    .ok_or_else(|| {
                        SidebandRunError::Admission(
                            "session activity admission is closed".to_string(),
                        )
                    })?
            }
        };
        let source_refs = match source {
            SidebandSource::None => Vec::new(),
            SidebandSource::Frozen(source_refs) => source_refs,
        };
        let parent_timeline_id = self.session_info.id.to_string();
        if let Some(source_ref) = source_refs
            .iter()
            .find(|source_ref| source_ref.timeline_id != parent_timeline_id)
        {
            return Err(chat_state::SidebandError::ForeignInputTimeline {
                expected: parent_timeline_id,
                actual: source_ref.timeline_id.clone(),
            }
            .into());
        }
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let mut timeline = chat_state::SidebandTimeline::new(sideband_id)?;
        let spawn_source_refs = source_refs.clone();
        let request = timeline.prepare(chat_state::SidebandEventKind::Request(
            chat_state::SidebandRequest {
                purpose,
                prompt,
                source_refs,
                budget_policy,
                route,
                initiator_ref: format!(
                    "t:{}/sideband:{}",
                    self.session_info.id,
                    timeline.sideband_id()
                ),
                executor: if self.startup_hints.is_subagent {
                    format!("subagent:{}", self.session_info.id)
                } else {
                    "main".into()
                },
                output_schema,
            },
        ))?;
        let mut run = SidebandRun {
            background: None,
            timeline,
            persistence: self.notifications.clone(),
            cancellation: if finalizer {
                self.finalizer_sideband_cancel.child_token()
            } else {
                self.sideband_cancel.child_token()
            },
            repair_cancellation: self.sideband_repair_cancel.child_token(),
            fail_stop: std::sync::Arc::clone(&self.sideband_fail_stop),
            goal_usage_window: self.goal_usage_window.clone(),
            usage_owner_id: self.session_id_string(),
            usage_epoch: super::tasks_cancel::turn_usage_epoch_or(
                self.goal_usage_window
                    .owner_epoch(&self.session_id_string()),
            ),
            expected_goal_id: self
                .events
                .current_goal_id()
                .or_else(|| self.goal_usage_window.active_goal_id()),
            admitted_attempt_id: None,
            pending: Some(request),
            persistence_poison: None,
            activity: Some(activity),
        };
        // Parent ownership must become durable before the independent child
        // ledger can contain a fact. A crash after this boundary may leave an
        // inert Spawn without a Request, which readers already ignore; the
        // reverse order leaves an unowned ledger that cannot be authenticated.
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Sideband(
                chat_state::SidebandSpawnEvent {
                    sideband_id: run.timeline.sideband_id().to_owned(),
                    purpose,
                    source_refs: spawn_source_refs,
                },
            ))
            .await?;
        run.flush_pending().await?;
        Ok(run)
    }
}

impl SidebandRun {
    /// Normalize one successful provider response. Goal accounting is applied
    /// by the root lifecycle mailbox so Goal mutation remains serialized with
    /// pause, edit, completion, and every other usage settlement.
    pub(crate) fn response_usage(
        &self,
        response: &ConversationResponse,
    ) -> chat_state::SidebandUsage {
        let usage = sideband_usage(response);
        usage
    }

    /// Settle the current provider attempt without transferring its ownership
    /// before the lifecycle root acknowledges it. If this future is aborted,
    /// `Drop` still sees the attempt id and performs fail-closed detached
    /// settlement; a successful acknowledgement disarms that fallback.
    pub(crate) async fn settle_goal_attempt(
        &mut self,
        tokens: Option<i64>,
    ) -> Result<(), SidebandRunError> {
        let Some(attempt_id) = self.admitted_attempt_id.clone() else {
            return Ok(());
        };
        self.goal_usage_window
            .settle_attempt_via_root(attempt_id.clone(), tokens)
            .await
            .map_err(SidebandRunError::Persistence)?;
        if self.admitted_attempt_id.as_deref() == Some(attempt_id.as_str()) {
            self.admitted_attempt_id = None;
        }
        Ok(())
    }

    fn claim_goal_attempt(&mut self, tokens: Option<i64>) -> Option<String> {
        let attempt_id = self.admitted_attempt_id.clone()?;
        if !self
            .goal_usage_window
            .claim_attempt_settlement(&attempt_id, tokens)
        {
            self.admitted_attempt_id = None;
            return None;
        }
        Some(attempt_id)
    }

    fn accept_goal_attempt_settlement(&mut self, attempt_id: &str) {
        if self.admitted_attempt_id.as_deref() == Some(attempt_id) {
            self.admitted_attempt_id = None;
        }
    }

    pub(crate) async fn attempt_all_sources(
        &mut self,
        request: &ConversationRequest,
        backend: sampling_types::ApiBackend,
        feedback: Option<String>,
    ) -> Result<(), SidebandRunError> {
        let source_refs = self
            .timeline
            .events()
            .first()
            .and_then(|event| match &event.kind {
                chat_state::SidebandEventKind::Request(request) => {
                    Some(request.source_refs.clone())
                }
                _ => None,
            })
            .ok_or(chat_state::SidebandError::AttemptWithoutRequest)?;
        self.attempt_selected(
            request,
            backend,
            source_refs,
            None,
            Vec::new(),
            Vec::new(),
            "all-sources",
            feedback,
        )
        .await
    }

    pub(crate) async fn attempt_selected(
        &mut self,
        request: &ConversationRequest,
        backend: sampling_types::ApiBackend,
        input_refs: Vec<chat_state::TimelineRangeRef>,
        source_revision: Option<u64>,
        context_surface_ids: Vec<chat_state::SurfaceId>,
        selected_surface_ids: Vec<chat_state::SurfaceId>,
        strategy: &str,
        feedback: Option<String>,
    ) -> Result<(), SidebandRunError> {
        if !request.tools.is_empty() || request.tool_choice.is_some() {
            let error = chat_state::SidebandError::ToolCapabilityForbidden;
            self.append(chat_state::SidebandEventKind::End(
                chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Failed,
                    error: Some(error.to_string()),
                },
            ))
            .await?;
            return Err(error.into());
        }
        let recorded_request = self
            .timeline
            .events()
            .first()
            .and_then(|event| match &event.kind {
                chat_state::SidebandEventKind::Request(request) => Some(request),
                _ => None,
            })
            .ok_or(chat_state::SidebandError::AttemptWithoutRequest)?;
        if request.model.as_deref() != Some(recorded_request.route.model.as_str())
            || backend != recorded_request.route.backend
        {
            let error = chat_state::SidebandError::RouteMismatch;
            self.append(chat_state::SidebandEventKind::End(
                chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Failed,
                    error: Some(error.to_string()),
                },
            ))
            .await?;
            return Err(error.into());
        }
        if !output_constraint_matches(recorded_request, request) {
            let error = chat_state::SidebandError::OutputConstraintMismatch;
            self.append(chat_state::SidebandEventKind::End(
                chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Failed,
                    error: Some(error.to_string()),
                },
            ))
            .await?;
            return Err(error.into());
        }
        let attempt_no = u32::try_from(
            self.timeline
                .events()
                .iter()
                .filter(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
                .count()
                .saturating_add(1),
        )
        .map_err(|_| chat_state::SidebandError::AttemptOverflow)?;
        self.settle_goal_attempt(None).await?;
        self.goal_usage_window
            .wait_for_owner_settlements_through(&self.usage_owner_id, self.usage_epoch)
            .await;
        let result = self
            .append(chat_state::SidebandEventKind::Attempt(
                chat_state::SidebandAttempt {
                    attempt_no,
                    input_refs,
                    assembly_manifest: chat_state::SidebandAssemblyManifest {
                        strategy: strategy.into(),
                        strategy_version: 1,
                        source_revision,
                        context_surface_ids,
                        selected_surface_ids,
                        materialized_input_tokens: chat_state::estimate_request_input_tokens(
                            request,
                        ),
                        max_output_tokens: request.max_output_tokens.map(u64::from),
                    },
                    feedback,
                },
            ))
            .await;
        result
    }

    /// Poll-bind the Goal admission lease to the provider future. If an outer
    /// cancellation/timeout branch wins before this future is polled, no Goal
    /// attempt exists. Admission can wait for prior settlements; once granted,
    /// it and the provider's first poll happen in the same task poll, so an
    /// unknown result is legitimately fail-closed rather than a pre-wire false
    /// positive.
    pub(crate) async fn run_provider<F: std::future::Future>(
        &mut self,
        provider: F,
    ) -> Result<F::Output, SidebandRunError> {
        let cancellation = self.cancellation.clone();
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(SidebandRunError::Cancelled),
            output = async {
                self.provider_attempt_started().await?;
                Ok(provider.await)
            } => output,
        };
        if let Some(id) = &self.admitted_attempt_id {
            self.goal_usage_window.mark_attempt_returned(id);
        }
        result
    }

    pub(crate) fn set_background(
        &mut self,
        background: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) {
        self.background = background;
    }

    async fn provider_attempt_started(&mut self) -> Result<(), SidebandRunError> {
        if self.admitted_attempt_id.is_some() {
            return Err(SidebandRunError::Admission(
                "sideband provider attempt already started".into(),
            ));
        }
        let has_open_attempt =
            self.timeline.events().last().is_some_and(|event| {
                matches!(event.kind, chat_state::SidebandEventKind::Attempt(_))
            });
        if !has_open_attempt {
            return Err(SidebandRunError::Admission(
                "sideband provider start has no durable attempt".into(),
            ));
        }
        self.admitted_attempt_id = self
            .goal_usage_window
            .begin_model_attempt_with_background(
                &self.usage_owner_id,
                self.usage_epoch,
                self.expected_goal_id.as_deref(),
                self.background.clone(),
            )
            .await
            .map_err(SidebandRunError::Admission)?;
        Ok(())
    }

    pub(crate) async fn complete(
        &mut self,
        raw_output: String,
        structured_output: Option<serde_json::Value>,
        usage: chat_state::SidebandUsage,
        finish: String,
        evidence_refs: Vec<chat_state::TimelineRangeRef>,
    ) -> Result<chat_state::TimelineRangeRef, SidebandRunError> {
        let attempt_seq = self
            .timeline
            .events()
            .iter()
            .rev()
            .find_map(|event| {
                matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)).then_some(event.seq)
            })
            .ok_or(chat_state::SidebandError::ResultWithoutAttempt)?;
        let result = chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
            raw_output,
            structured_output,
            usage,
            finish,
            source_event_seqs: [0, attempt_seq],
            evidence_refs,
        });
        if let Err(error) = self.append(result).await {
            if matches!(
                &error,
                SidebandRunError::Invalid(
                    chat_state::SidebandError::MissingStructuredOutput
                        | chat_state::SidebandError::StructuredOutputSchemaMismatch(_)
                )
            ) {
                self.append(chat_state::SidebandEventKind::End(
                    chat_state::SidebandEnd {
                        outcome: chat_state::SidebandOutcome::Failed,
                        error: Some(error.to_string()),
                    },
                ))
                .await?;
            }
            return Err(error);
        }
        let result_ref = chat_state::TimelineRangeRef {
            timeline_id: self.timeline.sideband_id().to_owned(),
            first_seq: attempt_seq + 1,
            last_seq: attempt_seq + 1,
        };
        self.append(chat_state::SidebandEventKind::End(
            chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Completed,
                error: None,
            },
        ))
        .await?;
        Ok(result_ref)
    }

    pub(crate) async fn fail(
        &mut self,
        outcome: chat_state::SidebandOutcome,
        error: impl Into<String>,
    ) -> Result<chat_state::TimelineRangeRef, SidebandRunError> {
        debug_assert_ne!(outcome, chat_state::SidebandOutcome::Completed);
        let error = error.into();
        let error = crate::util::truncate(error.trim(), 2_000).to_string();
        let terminal_seq = self.timeline.events().len() as u64;
        self.append(chat_state::SidebandEventKind::End(
            chat_state::SidebandEnd {
                outcome,
                error: Some(if error.is_empty() {
                    "sideband failed without diagnostic detail".into()
                } else {
                    error
                }),
            },
        ))
        .await?;
        Ok(chat_state::TimelineRangeRef {
            timeline_id: self.timeline.sideband_id().to_owned(),
            first_seq: terminal_seq,
            last_seq: terminal_seq,
        })
    }

    async fn append(
        &mut self,
        kind: chat_state::SidebandEventKind,
    ) -> Result<(), SidebandRunError> {
        if let Some(error) = self.persistence_poison.as_ref() {
            return Err(SidebandRunError::Persistence(error.clone()));
        }
        if self.pending.is_some() {
            self.flush_pending().await?;
            return Err(SidebandRunError::InterruptedAppendRecovered);
        }
        let event = self.timeline.prepare(kind)?;
        self.pending = Some(event);
        self.flush_pending().await
    }

    async fn flush_pending(&mut self) -> Result<(), SidebandRunError> {
        let Some(event) = self.pending.clone() else {
            return Ok(());
        };
        if let Err(error) =
            append_sideband_exact(&self.persistence, &self.cancellation, event.clone()).await
        {
            self.persistence_poison = Some(error.to_string());
            return Err(error);
        }
        self.timeline.accept(event)?;
        self.pending = None;
        Ok(())
    }
}

impl SessionActor {
    /// Settle a successful Sideband provider response through the same root /
    /// descendant authority split as regular model calls.
    pub(crate) async fn settle_sideband_response_usage(
        &self,
        run: &mut SidebandRun,
        response: &ConversationResponse,
    ) -> Result<chat_state::SidebandUsage, SidebandRunError> {
        let usage = run.response_usage(response);
        let tokens = response
            .usage
            .as_ref()
            .map(crate::session::goal_tracker::model_usage_goal_tokens);
        self.settle_sideband_attempt(run, tokens).await?;
        Ok(usage)
    }

    pub(crate) async fn settle_sideband_usage(
        &self,
        run: &mut SidebandRun,
        usage: &chat_state::SidebandUsage,
    ) -> Result<(), SidebandRunError> {
        self.settle_sideband_attempt(run, Some(sideband_goal_tokens(usage)))
            .await
    }

    pub(crate) async fn settle_sideband_attempt_incomplete(
        &self,
        run: &mut SidebandRun,
    ) -> Result<(), SidebandRunError> {
        self.settle_sideband_attempt(run, None).await
    }

    async fn settle_sideband_attempt(
        &self,
        run: &mut SidebandRun,
        tokens: Option<i64>,
    ) -> Result<(), SidebandRunError> {
        if self.startup_hints.is_subagent {
            return run.settle_goal_attempt(tokens).await;
        }
        let Some(attempt_id) = run.claim_goal_attempt(tokens) else {
            return Ok(());
        };
        self.settle_claimed_goal_usage_attempt(&attempt_id)
            .await
            .map_err(SidebandRunError::Persistence)?;
        run.accept_goal_attempt_settlement(&attempt_id);
        Ok(())
    }
}

impl Drop for SidebandRun {
    fn drop(&mut self) {
        if let Some(attempt_id) = self.admitted_attempt_id.take() {
            self.goal_usage_window
                .settle_attempt_detached(attempt_id, None);
        }
        if self.timeline.is_ended() {
            return;
        }
        let activity = self.activity.take();
        if self.cancellation.is_cancelled()
            && self.fail_stop.load(std::sync::atomic::Ordering::Acquire)
        {
            // Fatal/final teardown has revoked the persistence epoch. Starting
            // an independent repair writer here would cross the final flush.
            // The open ledger is an explicit fail-stop artifact instead.
            return;
        }
        let mut timeline = self.timeline.clone();
        let pending = self.pending.take();
        let persistence = self.persistence.clone();
        let cancellation = self.repair_cancellation.clone();
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!(
                sideband_id = %timeline.sideband_id(),
                "open Sideband owner dropped outside its runtime; terminal lease unavailable"
            );
            return;
        };
        runtime.spawn(async move {
            let _activity = activity;
            // The foreground future may have been aborted after enqueueing an
            // immutable event but before receiving its acknowledgement. Replay
            // that exact event first; persistence deduplicates it by identity.
            if let Some(event) = pending {
                if let Err(error) =
                    append_sideband_exact(&persistence, &cancellation, event.clone()).await
                {
                    tracing::error!(%error, "failed to settle interrupted Sideband append");
                    return;
                }
                if let Err(error) = timeline.accept(event) {
                    tracing::error!(%error, "interrupted Sideband append violated its local fold");
                    return;
                }
            }
            if timeline.is_ended() {
                return;
            }
            let terminal = match timeline.prepare(chat_state::SidebandEventKind::End(
                chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Cancelled,
                    error: Some("sideband owner dropped before terminal settlement".into()),
                },
            )) {
                Ok(terminal) => terminal,
                Err(error) => {
                    tracing::error!(%error, "failed to prepare interrupted Sideband terminal");
                    return;
                }
            };
            if let Err(error) = append_sideband_exact(&persistence, &cancellation, terminal).await {
                tracing::error!(%error, "failed to persist interrupted Sideband terminal");
            }
        });
    }
}

fn sideband_goal_tokens(usage: &chat_state::SidebandUsage) -> i64 {
    let uncached_input = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
    i64::try_from(uncached_input.saturating_add(usage.output_tokens)).unwrap_or(i64::MAX)
}

fn output_constraint_matches(
    recorded: &chat_state::SidebandRequest,
    provider: &ConversationRequest,
) -> bool {
    match (&recorded.output_schema, &provider.json_output) {
        (None, None) => true,
        (Some(schema), Some(actual)) => {
            actual
                == &sampling_types::JsonOutputFormat::portable_schema_for_backend(
                    recorded.route.backend.clone(),
                    schema.clone(),
                )
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}

async fn append_sideband_exact(
    persistence: &NotificationSender,
    cancellation: &tokio_util::sync::CancellationToken,
    event: chat_state::SidebandEvent,
) -> Result<(), SidebandRunError> {
    let mut attempts = 0_u32;
    loop {
        let append = persistence.append_sideband_event_durably(event.clone());
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(SidebandRunError::Cancelled),
            result = append => result,
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.retry_exact() => {
                attempts = attempts.saturating_add(1);
                if attempts == 1 || attempts % 10 == 0 {
                    tracing::warn!(
                        sideband_id = %event.sideband_id,
                        seq = event.seq,
                        attempts,
                        %error,
                        "sideband durability is uncertain; retrying the exact immutable event"
                    );
                }
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        return Err(SidebandRunError::Cancelled);
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
            }
            Err(error) => return Err(SidebandRunError::Persistence(error.to_string())),
        }
    }
}

pub(crate) fn sideband_usage(response: &ConversationResponse) -> chat_state::SidebandUsage {
    response.usage.as_ref().map_or_else(
        chat_state::SidebandUsage::default,
        sideband_usage_from_tokens,
    )
}

pub(crate) fn sideband_usage_from_tokens(
    usage: &sampling_types::TokenUsage,
) -> chat_state::SidebandUsage {
    chat_state::SidebandUsage {
        input_tokens: usage.prompt_tokens.into(),
        output_tokens: usage.completion_tokens.into(),
        cache_read_tokens: usage.cached_prompt_tokens.into(),
        cache_write_tokens: usage.cache_creation_prompt_tokens.into(),
    }
}

pub(crate) fn sideband_finish(response: &ConversationResponse) -> String {
    response
        .stop_reason
        .map(|reason| reason.as_str().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_run_with_schema(
        output_schema: Option<serde_json::Value>,
    ) -> (
        SidebandRun,
        tokio::sync::mpsc::UnboundedReceiver<crate::session::persistence::PersistenceMsg>,
    ) {
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let mut timeline = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
        let request = timeline
            .prepare(chat_state::SidebandEventKind::Request(
                chat_state::SidebandRequest {
                    purpose: chat_state::SidebandPurpose::PermissionJudgment,
                    prompt: "judge".into(),
                    source_refs: Vec::new(),
                    budget_policy: chat_state::SidebandBudgetPolicy {
                        max_attempts: 2,
                        max_input_tokens_per_attempt: 32,
                        max_output_tokens_per_attempt: None,
                    },
                    route: chat_state::SidebandRoute {
                        model: "test-model".into(),
                        backend: sampling_types::ApiBackend::Responses,
                    },
                    initiator_ref: format!("t:parent/sideband:{sideband_id}"),
                    executor: "main".into(),
                    output_schema,
                },
            ))
            .unwrap();
        timeline.accept(request).unwrap();

        let (persistence_tx, persistence_rx) = tokio::sync::mpsc::unbounded_channel();
        let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
        let persistence = NotificationSender {
            gateway: acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            gateway_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            persistence_tx,
        };
        let (goal_tx, _goal_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            SidebandRun {
                background: None,
                timeline,
                persistence,
                cancellation: tokio_util::sync::CancellationToken::new(),
                repair_cancellation: tokio_util::sync::CancellationToken::new(),
                fail_stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                goal_usage_window: crate::session::actor::goal_support::GoalUsageWindow::new(
                    goal_tx, None,
                ),
                usage_owner_id: "test-session".into(),
                usage_epoch: 0,
                expected_goal_id: None,
                admitted_attempt_id: None,
                pending: None,
                persistence_poison: None,
                activity: None,
            },
            persistence_rx,
        )
    }

    fn test_run() -> (
        SidebandRun,
        tokio::sync::mpsc::UnboundedReceiver<crate::session::persistence::PersistenceMsg>,
    ) {
        test_run_with_schema(None)
    }

    fn provider_request() -> ConversationRequest {
        ConversationRequest {
            model: Some("test-model".into()),
            ..ConversationRequest::default()
        }
    }

    fn provider_response_with_usage() -> ConversationResponse {
        ConversationResponse {
            items: vec![sampling_types::ConversationItem::assistant("ok")],
            stop_reason: None,
            usage: Some(sampling_types::TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20,
                total_tokens: 120,
                reasoning_tokens: 5,
                cached_prompt_tokens: 40,
                cache_creation_prompt_tokens: 0,
            }),
            cost_usd_ticks: None,
            message_chunks_emitted: 1,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: None,
            stop_sequence: None,
        }
    }

    fn terminal_event(run: &mut SidebandRun) -> chat_state::SidebandEvent {
        run.timeline
            .prepare(chat_state::SidebandEventKind::End(
                chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Failed,
                    error: Some("test".into()),
                },
            ))
            .unwrap()
    }

    fn goal_usage_mailbox(
        actor: &std::sync::Arc<SessionActor>,
        active_goal_id: Option<String>,
    ) -> crate::session::actor::goal_support::GoalUsageWindow {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let window =
            crate::session::actor::goal_support::GoalUsageWindow::new(tx.clone(), active_goal_id);
        let actor = actor.clone();
        let mailbox_window = window.clone();
        tokio::task::spawn_local(async move {
            let _keepalive = tx;
            while let Some(command) = rx.recv().await {
                match command {
                    crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                        attempt_id,
                        respond_to,
                    } => {
                        let result = match mailbox_window.attempt_settlement(&attempt_id) {
                            Some((goal_id, Some(tokens))) => {
                                actor.apply_captured_goal_usage(&goal_id, tokens).await
                            }
                            Some((goal_id, None)) => {
                                actor.apply_captured_goal_usage_incomplete(&goal_id).await
                            }
                            None => Ok(false),
                        };
                        if result.is_ok() {
                            mailbox_window.finish_attempt(&attempt_id);
                        }
                        let _ = respond_to.send(result);
                    }
                    crate::session::commands::SessionCommand::RecordGoalUsage {
                        goal_id,
                        tokens,
                        respond_to,
                    } => {
                        let _ = respond_to
                            .send(actor.apply_captured_goal_usage(&goal_id, tokens).await);
                    }
                    crate::session::commands::SessionCommand::RecordGoalUsageIncomplete {
                        goal_id,
                        respond_to,
                    } => {
                        let _ = respond_to
                            .send(actor.apply_captured_goal_usage_incomplete(&goal_id).await);
                    }
                    _ => panic!("unexpected command on Goal usage test mailbox"),
                }
            }
        });
        window
    }

    #[tokio::test]
    async fn permanent_persistence_error_does_not_retry_forever() {
        let (mut run, mut persistence_rx) = test_run();
        let event = terminal_event(&mut run);
        let mut append = Box::pin(append_sideband_exact(
            &run.persistence,
            &run.cancellation,
            event,
        ));

        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut append => panic!("append returned before persistence replied: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to
            .send(Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "disk full",
            )))
            .unwrap();

        assert!(matches!(
            append.await,
            Err(SidebandRunError::Persistence(error)) if error.contains("disk full")
        ));
        assert!(persistence_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn transient_persistence_retry_stops_at_session_cancellation() {
        let (mut run, mut persistence_rx) = test_run();
        let event = terminal_event(&mut run);
        let cancellation = run.cancellation.clone();
        let mut append = Box::pin(append_sideband_exact(
            &run.persistence,
            &run.cancellation,
            event,
        ));

        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut append => panic!("append returned before persistence replied: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to
            .send(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "retry",
            )))
            .unwrap();
        cancellation.cancel();

        assert!(matches!(append.await, Err(SidebandRunError::Cancelled)));
    }

    #[tokio::test]
    async fn sideband_usage_uses_the_goal_window_at_attempt_admission() {
        let (mut run, _persistence_rx) = test_run();
        let (goal_tx, _goal_rx) = tokio::sync::mpsc::unbounded_channel();
        let window = crate::session::actor::goal_support::GoalUsageWindow::new(
            goal_tx,
            Some("goal-1".into()),
        );
        run.goal_usage_window = window.clone();
        let admitted = window
            .begin_model_attempt("test-session", 0, Some("goal-1"))
            .await
            .unwrap()
            .unwrap();
        run.admitted_attempt_id = Some(admitted.clone());
        window.sync(None);
        assert_eq!(
            window.attempt_goal_id(&admitted).as_deref(),
            Some("goal-1"),
            "pausing after admission must not erase the immutable Goal owner"
        );
        window.finish_attempt(&admitted);
        run.admitted_attempt_id = None;

        assert_eq!(
            window
                .begin_model_attempt("test-session", 0, None)
                .await
                .unwrap(),
            None
        );
        assert!(
            window
                .begin_model_attempt("test-session", 0, Some("goal-1"))
                .await
                .is_err(),
            "a turn admitted under a closed Goal cannot start another provider attempt"
        );
        window.sync(Some("goal-1".into()));
        let restarted = window
            .begin_model_attempt("test-session", 0, Some("goal-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            window.attempt_goal_id(&restarted).as_deref(),
            Some("goal-1")
        );
        window.finish_attempt(&restarted);
    }

    #[tokio::test]
    async fn durable_attempt_does_not_enter_goal_usage_until_provider_start() {
        let (mut run, mut persistence_rx) = test_run();
        let (goal_tx, _goal_rx) = tokio::sync::mpsc::unbounded_channel();
        let window = crate::session::actor::goal_support::GoalUsageWindow::new(
            goal_tx,
            Some("goal-1".into()),
        );
        run.goal_usage_window = window.clone();
        run.expected_goal_id = Some("goal-1".into());

        let request = provider_request();
        let mut attempt = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            None,
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.expect("durable attempt append"),
            result = &mut attempt => panic!("attempt returned before persistence ack: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to.send(Ok(())).unwrap();
        attempt.await.unwrap();

        assert!(
            run.admitted_attempt_id.is_none(),
            "a durable plan is not yet a provider call"
        );
        run.provider_attempt_started().await.unwrap();
        let attempt_id = run
            .admitted_attempt_id
            .clone()
            .expect("provider start enters Goal accounting");
        assert_eq!(
            window.attempt_goal_id(&attempt_id).as_deref(),
            Some("goal-1")
        );
        window.finish_attempt(&attempt_id);
        run.admitted_attempt_id = None;
    }

    #[tokio::test]
    async fn pre_cancelled_provider_future_never_enters_goal_usage() {
        let (mut run, mut persistence_rx) = test_run();
        let (goal_tx, _goal_rx) = tokio::sync::mpsc::unbounded_channel();
        let window = crate::session::actor::goal_support::GoalUsageWindow::new(
            goal_tx,
            Some("goal-1".into()),
        );
        run.goal_usage_window = window.clone();
        run.expected_goal_id = Some("goal-1".into());
        let request = provider_request();
        let mut attempt = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            None,
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.expect("durable attempt append"),
            result = &mut attempt => panic!("attempt returned before persistence ack: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to.send(Ok(())).unwrap();
        attempt.await.unwrap();

        let cancelled = tokio_util::sync::CancellationToken::new();
        cancelled.cancel();
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => {}
            result = run.run_provider(std::future::pending::<()>()) => {
                panic!("pre-cancelled provider future was polled: {result:?}")
            }
        }

        assert!(run.admitted_attempt_id.is_none());
        let probe = window
            .begin_model_attempt("test-session", 0, Some("goal-1"))
            .await
            .expect("pre-cancellation must not leave a hidden attempt fence")
            .expect("active Goal must still admit a provider attempt");
        window.finish_attempt(&probe);
    }

    #[tokio::test]
    async fn cancellation_during_final_admission_never_polls_sideband_provider() {
        let (mut run, mut persistence_rx) = test_run();
        let (goal_tx, _goal_rx) = tokio::sync::mpsc::unbounded_channel();
        let window = crate::session::actor::goal_support::GoalUsageWindow::new(
            goal_tx,
            Some("goal-1".into()),
        );
        run.goal_usage_window = window.clone();
        run.expected_goal_id = Some("goal-1".into());
        let prior = window
            .begin_model_attempt_with_background(
                &run.usage_owner_id,
                run.usage_epoch,
                Some("goal-1"),
                Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    true,
                ))),
            )
            .await
            .unwrap()
            .unwrap();
        let request = provider_request();
        let mut attempt = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            None,
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.expect("durable attempt append"),
            result = &mut attempt => panic!("attempt returned before persistence ack: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to.send(Ok(())).unwrap();
        attempt.await.unwrap();

        // Return after the early fence and durable Attempt, before provider poll.
        window.mark_attempt_returned(&prior);
        let cancelled = run.cancellation.clone();
        let polled = std::sync::atomic::AtomicBool::new(false);
        let mut provider = Box::pin(run.run_provider(async {
            polled.store(true, std::sync::atomic::Ordering::Release);
        }));
        tokio::select! {
            biased;
            result = &mut provider => panic!("unconfirmed usage must block: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }
        cancelled.cancel();
        assert!(matches!(provider.await, Err(SidebandRunError::Cancelled)));
        assert!(!polled.load(std::sync::atomic::Ordering::Acquire));
        assert!(run.admitted_attempt_id.is_none());
        assert!(window.finish_attempt(&prior));
        let probe = window
            .begin_model_attempt(&run.usage_owner_id, run.usage_epoch, Some("goal-1"))
            .await
            .unwrap()
            .unwrap();
        window.finish_attempt(&probe);
    }

    #[tokio::test]
    async fn dropping_an_open_sideband_persists_one_cancelled_terminal() {
        let (run, mut persistence_rx) = test_run();
        drop(run);

        let message =
            tokio::time::timeout(std::time::Duration::from_secs(1), persistence_rx.recv())
                .await
                .expect("Sideband terminal lease must run independently of its owner")
                .expect("persistence channel must remain open until the terminal is sent");
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event,
            respond_to,
        } = message
        else {
            panic!("unexpected persistence message");
        };
        assert!(matches!(
            event.kind,
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Cancelled,
                ..
            })
        ));
        respond_to.send(Ok(())).unwrap();

        assert!(
            !matches!(
                tokio::time::timeout(std::time::Duration::from_millis(20), persistence_rx.recv())
                    .await,
                Ok(Some(_))
            ),
            "Drop must enqueue exactly one terminal"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborting_usage_settlement_keeps_the_attempt_owned_until_drop_fails_closed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut run, _persistence_rx) = test_run();
                let (goal_tx, mut goal_rx) = tokio::sync::mpsc::unbounded_channel();
                let _goal_mailbox_owner = goal_tx.clone();
                let window = crate::session::actor::goal_support::GoalUsageWindow::new(
                    goal_tx,
                    Some("goal-1".into()),
                );
                let attempt_id = window
                    .begin_model_attempt("test-session", 0, Some("goal-1"))
                    .await
                    .unwrap()
                    .unwrap();
                run.goal_usage_window = window.clone();
                run.admitted_attempt_id = Some(attempt_id.clone());

                let settlement =
                    tokio::task::spawn_local(
                        async move { run.settle_goal_attempt(Some(80)).await },
                    );
                let first = goal_rx.recv().await.expect("known-usage settlement");
                let crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id: first_attempt_id,
                    respond_to: first_respond_to,
                } = first
                else {
                    panic!("expected known-usage settlement");
                };
                assert_eq!(first_attempt_id, attempt_id);
                assert_eq!(
                    window.attempt_settlement(&attempt_id),
                    Some(("goal-1".into(), Some(80)))
                );
                settlement.abort();
                assert!(settlement.await.unwrap_err().is_cancelled());
                drop(first_respond_to);

                // Dropping the aborted future also drops SidebandRun. Because
                // the root acknowledgement never disarmed its local attempt
                // ownership, Drop must retry the exact already-claimed Known
                // result rather than degrading it to usage-incomplete.
                let second = goal_rx.recv().await.expect("detached exact settlement");
                let crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id: second_attempt_id,
                    respond_to,
                } = second
                else {
                    panic!("aborted settlement must retry the claimed attempt");
                };
                assert_eq!(second_attempt_id, attempt_id);
                assert_eq!(
                    window.attempt_settlement(&attempt_id),
                    Some(("goal-1".into(), Some(80)))
                );
                window.finish_attempt(&attempt_id);
                respond_to.send(Ok(true)).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    while window.attempt_goal_id(&attempt_id).is_some() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("detached settlement must release the exact attempt");
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn root_sideband_usage_uses_the_in_process_lifecycle_authority() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = std::sync::Arc::new(
                    crate::session::actor::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                actor
                    .goal_tracker
                    .lock()
                    .create_goal("goal-1".into(), "finish".into(), Some(10_000), "now".into())
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();
                let (mut run, _rx) = test_run();
                run.goal_usage_window = actor.goal_usage_window.clone();
                run.admitted_attempt_id = run
                    .goal_usage_window
                    .begin_model_attempt(&run.usage_owner_id, run.usage_epoch, Some("goal-1"))
                    .await
                    .unwrap();
                tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    actor.settle_sideband_response_usage(&mut run, &provider_response_with_usage()),
                )
                .await
                .expect("root Sideband settlement must reach the lifecycle authority")
                .unwrap();

                assert_eq!(actor.goal_tokens_used(), 80);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_sideband_without_provider_usage_pauses_goal_fail_closed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = std::sync::Arc::new(
                    crate::session::actor::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                actor
                    .goal_tracker
                    .lock()
                    .create_goal("goal-1".into(), "finish".into(), Some(10_000), "now".into())
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();

                let (mut run, _rx) = test_run();
                run.goal_usage_window = actor.goal_usage_window.clone();
                run.admitted_attempt_id = run
                    .goal_usage_window
                    .begin_model_attempt(&run.usage_owner_id, run.usage_epoch, Some("goal-1"))
                    .await
                    .unwrap();
                let mut response = provider_response_with_usage();
                response.usage = None;
                actor
                    .settle_sideband_response_usage(&mut run, &response)
                    .await
                    .unwrap();

                let snapshot = actor.goal_tracker.lock().snapshot().cloned().unwrap();
                assert!(snapshot.usage_incomplete);
                assert_eq!(
                    snapshot.status,
                    crate::session::goal_tracker::GoalStatus::Paused
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Normal,
                    "fail-closed usage must release Goal Behavior atomically"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn cancelled_append_recovers_exact_event_without_swallowing_new_operation() {
        let (mut run, mut persistence_rx) = test_run();
        let request = provider_request();

        let mut first = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            Some("first".into()),
        ));
        let first_message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut first => panic!("append unexpectedly completed: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event: first_event,
            respond_to,
        } = first_message
        else {
            panic!("unexpected persistence message");
        };
        drop(first);
        drop(respond_to);

        let mut retry = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            Some("different".into()),
        ));
        let retry_message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut retry => panic!("retry unexpectedly completed: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event: retry_event,
            respond_to,
        } = retry_message
        else {
            panic!("unexpected persistence message");
        };
        assert_eq!(
            serde_json::to_vec(&retry_event).unwrap(),
            serde_json::to_vec(&first_event).unwrap()
        );
        respond_to.send(Ok(())).unwrap();
        assert!(matches!(
            retry.await,
            Err(SidebandRunError::InterruptedAppendRecovered)
        ));

        assert_eq!(
            run.timeline
                .events()
                .iter()
                .filter(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn tool_capability_is_rejected_before_attempt_persistence() {
        let requests = [
            ConversationRequest {
                tools: vec![sampling_types::ToolSpec {
                    name: "read_file".into(),
                    description: None,
                    parameters: serde_json::json!({"type": "object"}),
                }],
                ..provider_request()
            },
            ConversationRequest {
                tool_choice: Some(sampling_types::ConversationToolChoice::None),
                ..provider_request()
            },
        ];

        for request in requests {
            let (mut run, mut persistence_rx) = test_run();
            let mut rejected = Box::pin(run.attempt_all_sources(
                &request,
                sampling_types::ApiBackend::Responses,
                None,
            ));
            let message = tokio::select! {
                message = persistence_rx.recv() => message.unwrap(),
                result = &mut rejected => panic!("rejection returned before its terminal fact was durable: {result:?}"),
            };
            let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
                event,
                respond_to,
            } = message
            else {
                panic!("unexpected persistence message");
            };
            assert!(matches!(
                event.kind,
                chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Failed,
                    ..
                })
            ));
            respond_to.send(Ok(())).unwrap();
            assert!(matches!(
                rejected.await,
                Err(SidebandRunError::Invalid(
                    chat_state::SidebandError::ToolCapabilityForbidden
                ))
            ));
            assert_eq!(
                run.timeline
                    .events()
                    .iter()
                    .filter(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
                    .count(),
                0
            );
            assert!(matches!(
                run.timeline.events().last().map(|event| &event.kind),
                Some(chat_state::SidebandEventKind::End(
                    chat_state::SidebandEnd {
                        outcome: chat_state::SidebandOutcome::Failed,
                        ..
                    }
                ))
            ));
        }
    }

    #[test]
    fn provider_output_constraint_must_equal_the_durable_contract() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["decision"],
            "properties": { "decision": { "type": "string" } }
        });
        let request = chat_state::SidebandRequest {
            purpose: chat_state::SidebandPurpose::PermissionJudgment,
            prompt: "judge".into(),
            source_refs: Vec::new(),
            budget_policy: chat_state::SidebandBudgetPolicy {
                max_attempts: 1,
                max_input_tokens_per_attempt: 32,
                max_output_tokens_per_attempt: None,
            },
            route: chat_state::SidebandRoute {
                model: "test-model".into(),
                backend: sampling_types::ApiBackend::Responses,
            },
            initiator_ref: "parent".into(),
            executor: "main".into(),
            output_schema: Some(schema.clone()),
        };

        assert!(output_constraint_matches(
            &request,
            &ConversationRequest {
                json_output: Some(sampling_types::JsonOutputFormat::JsonSchema(schema.clone())),
                ..ConversationRequest::default()
            }
        ));
        assert!(!output_constraint_matches(
            &request,
            &ConversationRequest::default()
        ));
        assert!(!output_constraint_matches(
            &request,
            &ConversationRequest {
                json_output: Some(sampling_types::JsonOutputFormat::JsonSchema(
                    serde_json::json!({"type": "array"}),
                )),
                ..ConversationRequest::default()
            }
        ));

        let mut chat_request = request;
        chat_request.route.backend = sampling_types::ApiBackend::ChatCompletions;
        assert!(output_constraint_matches(
            &chat_request,
            &ConversationRequest {
                json_output: Some(sampling_types::JsonOutputFormat::JsonObject),
                ..ConversationRequest::default()
            }
        ));
    }

    #[tokio::test]
    async fn output_constraint_mismatch_is_terminal_before_attempt_persistence() {
        let (mut run, mut persistence_rx) =
            test_run_with_schema(Some(serde_json::json!({"type": "object"})));
        let request = provider_request();
        let mut rejected = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            None,
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut rejected => panic!("rejection returned before its terminal fact was durable: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event,
            respond_to,
        } = message
        else {
            panic!("unexpected persistence message");
        };
        assert!(matches!(
            event.kind,
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Failed,
                ..
            })
        ));
        respond_to.send(Ok(())).unwrap();
        assert!(matches!(
            rejected.await,
            Err(SidebandRunError::Invalid(
                chat_state::SidebandError::OutputConstraintMismatch
            ))
        ));
        assert_eq!(
            run.timeline
                .events()
                .iter()
                .filter(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn route_mismatch_is_terminal_before_attempt_persistence() {
        let (mut run, mut persistence_rx) = test_run();
        let request = provider_request();
        let mut rejected =
            Box::pin(run.attempt_all_sources(&request, sampling_types::ApiBackend::Messages, None));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut rejected => panic!("rejection returned before its terminal fact was durable: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event,
            respond_to,
        } = message
        else {
            panic!("unexpected persistence message");
        };
        assert!(matches!(
            event.kind,
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Failed,
                ..
            })
        ));
        respond_to.send(Ok(())).unwrap();
        assert!(matches!(
            rejected.await,
            Err(SidebandRunError::Invalid(
                chat_state::SidebandError::RouteMismatch
            ))
        ));
        assert!(
            !run.timeline
                .events()
                .iter()
                .any(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
        );
    }

    #[tokio::test]
    async fn impossible_schema_result_is_terminal_without_a_result_fact() {
        let schema = serde_json::json!({"type": "object"});
        let (mut run, mut persistence_rx) = test_run_with_schema(Some(schema.clone()));
        let request = ConversationRequest {
            model: Some("test-model".into()),
            json_output: Some(sampling_types::JsonOutputFormat::JsonSchema(schema)),
            ..ConversationRequest::default()
        };

        let mut attempted = Box::pin(run.attempt_all_sources(
            &request,
            sampling_types::ApiBackend::Responses,
            None,
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut attempted => panic!("attempt returned before its fact was durable: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            respond_to, ..
        } = message
        else {
            panic!("unexpected persistence message");
        };
        respond_to.send(Ok(())).unwrap();
        attempted.await.unwrap();

        let mut rejected = Box::pin(run.complete(
            "[]".into(),
            Some(serde_json::json!([])),
            chat_state::SidebandUsage::default(),
            "stop".into(),
            Vec::new(),
        ));
        let message = tokio::select! {
            message = persistence_rx.recv() => message.unwrap(),
            result = &mut rejected => panic!("rejection returned before its terminal fact was durable: {result:?}"),
        };
        let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
            event,
            respond_to,
        } = message
        else {
            panic!("unexpected persistence message");
        };
        assert!(matches!(
            event.kind,
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Failed,
                ..
            })
        ));
        respond_to.send(Ok(())).unwrap();
        assert!(matches!(
            rejected.await,
            Err(SidebandRunError::Invalid(
                chat_state::SidebandError::StructuredOutputSchemaMismatch(_)
            ))
        ));
        assert!(
            !run.timeline
                .events()
                .iter()
                .any(|event| matches!(event.kind, chat_state::SidebandEventKind::Result(_)))
        );
    }
}
