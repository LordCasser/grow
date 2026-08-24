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
    #[error("an interrupted sideband append was recovered; the new operation was not applied")]
    InterruptedAppendRecovered,
}

pub(crate) struct SidebandRun {
    timeline: chat_state::SidebandTimeline,
    persistence: NotificationSender,
    pending: Option<chat_state::SidebandEvent>,
    persistence_poison: Option<String>,
}

impl SessionActor {
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
            timeline,
            persistence: self.notifications.clone(),
            pending: Some(request),
            persistence_poison: None,
        };
        run.flush_pending().await?;
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Sideband(
                chat_state::SidebandSpawnEvent {
                    sideband_id: run.timeline.sideband_id().to_owned(),
                    purpose,
                    source_refs: run
                        .timeline
                        .events()
                        .first()
                        .and_then(|event| match &event.kind {
                            chat_state::SidebandEventKind::Request(request) => {
                                Some(request.source_refs.clone())
                            }
                            _ => None,
                        })
                        .ok_or(chat_state::SidebandError::InvalidRequestBoundary)?,
                },
            ))
            .await?;
        Ok(run)
    }
}

impl SidebandRun {
    pub(crate) async fn attempt_all_sources(
        &mut self,
        request: &ConversationRequest,
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
        input_refs: Vec<chat_state::TimelineRangeRef>,
        source_revision: Option<u64>,
        context_surface_ids: Vec<chat_state::SurfaceId>,
        selected_surface_ids: Vec<chat_state::SurfaceId>,
        strategy: &str,
        feedback: Option<String>,
    ) -> Result<(), SidebandRunError> {
        let attempt_no = u32::try_from(
            self.timeline
                .events()
                .iter()
                .filter(|event| matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)))
                .count()
                .saturating_add(1),
        )
        .map_err(|_| chat_state::SidebandError::AttemptOverflow)?;
        self.append(chat_state::SidebandEventKind::Attempt(
            chat_state::SidebandAttempt {
                attempt_no,
                input_refs,
                assembly_manifest: chat_state::SidebandAssemblyManifest {
                    strategy: strategy.into(),
                    strategy_version: 1,
                    source_revision,
                    context_surface_ids,
                    selected_surface_ids,
                    materialized_input_tokens: chat_state::estimate_request_input_tokens(request),
                    max_output_tokens: request.max_output_tokens.map(u64::from),
                },
                feedback,
            },
        ))
        .await
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
        self.append(chat_state::SidebandEventKind::Result(
            chat_state::SidebandResult {
                raw_output,
                structured_output,
                usage,
                finish,
                source_event_seqs: [0, attempt_seq],
                evidence_refs,
            },
        ))
        .await?;
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
        if let Err(error) = append_sideband_exact(&self.persistence, event.clone()).await {
            self.persistence_poison = Some(error.to_string());
            return Err(error);
        }
        self.timeline.accept(event)?;
        self.pending = None;
        Ok(())
    }
}

async fn append_sideband_exact(
    persistence: &NotificationSender,
    event: chat_state::SidebandEvent,
) -> Result<(), SidebandRunError> {
    let mut attempts = 0_u32;
    loop {
        match persistence
            .append_sideband_event_durably(event.clone())
            .await
        {
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
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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

pub(crate) fn sideband_backend(backend: sampling_types::ApiBackend) -> &'static str {
    match backend {
        sampling_types::ApiBackend::ChatCompletions => "chat_completions",
        sampling_types::ApiBackend::Responses => "responses",
        sampling_types::ApiBackend::Messages => "messages",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_run() -> (
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
                        backend: "responses".into(),
                    },
                    initiator_ref: format!("t:parent/sideband:{sideband_id}"),
                    executor: "main".into(),
                    output_schema: None,
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
        (
            SidebandRun {
                timeline,
                persistence,
                pending: None,
                persistence_poison: None,
            },
            persistence_rx,
        )
    }

    #[tokio::test]
    async fn cancelled_append_recovers_exact_event_without_swallowing_new_operation() {
        let (mut run, mut persistence_rx) = test_run();
        let request = ConversationRequest::default();

        let mut first = Box::pin(run.attempt_all_sources(&request, Some("first".into())));
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

        let mut retry = Box::pin(run.attempt_all_sources(&request, Some("different".into())));
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
}
