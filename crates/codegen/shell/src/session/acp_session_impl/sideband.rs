//! Durable sideband lifecycle shared by every auxiliary model call.

use sampling_types::ConversationResponse;

use crate::session::SessionActor;
use crate::session::notifications::NotificationSender;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SidebandInput {
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
}

pub(crate) struct SidebandRun {
    timeline: chat_state::SidebandTimeline,
    persistence: NotificationSender,
}

impl SessionActor {
    /// Commit a spawn fact and the sideband request before the caller may emit
    /// the auxiliary provider request. `Frozen` ranges were materialized in one
    /// parent-actor query, so later parent appends cannot expand them.
    pub(crate) async fn begin_sideband(
        &self,
        purpose: chat_state::SidebandPurpose,
        prompt: String,
        input: SidebandInput,
        route: chat_state::SidebandRoute,
        output_schema: Option<serde_json::Value>,
    ) -> Result<SidebandRun, SidebandRunError> {
        let input_refs = match input {
            SidebandInput::None => Vec::new(),
            SidebandInput::Frozen(input_refs) => input_refs,
        };
        let parent_timeline_id = self.session_info.id.to_string();
        if let Some(input_ref) = input_refs
            .iter()
            .find(|input_ref| input_ref.timeline_id != parent_timeline_id)
        {
            return Err(chat_state::SidebandError::ForeignInputTimeline {
                expected: parent_timeline_id,
                actual: input_ref.timeline_id.clone(),
            }
            .into());
        }
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let spawn = self
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Sideband(
                chat_state::SidebandSpawnEvent {
                    sideband_id: sideband_id.clone(),
                    purpose,
                    input_refs: input_refs.clone(),
                },
            ))
            .await?;
        let initiator_ref = format!("t:{}/{}", self.session_info.id, spawn.seq.get());
        let mut timeline = chat_state::SidebandTimeline::new(sideband_id)?;
        let request = timeline.prepare(chat_state::SidebandEventKind::Request(
            chat_state::SidebandRequest {
                purpose,
                prompt,
                input_refs,
                route,
                initiator_ref,
                executor: if self.startup_hints.is_subagent {
                    format!("subagent:{}", self.session_info.id)
                } else {
                    "main".into()
                },
                output_schema,
            },
        ))?;
        append_sideband_exact(&self.notifications, request.clone()).await?;
        timeline.accept(request)?;
        Ok(SidebandRun {
            timeline,
            persistence: self.notifications.clone(),
        })
    }
}

impl SidebandRun {
    pub(crate) async fn attempt(
        &mut self,
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
        let event = self.timeline.prepare(kind)?;
        append_sideband_exact(&self.persistence, event.clone()).await?;
        self.timeline.accept(event)?;
        Ok(())
    }
}

async fn append_sideband_exact(
    persistence: &NotificationSender,
    event: chat_state::SidebandEvent,
) -> Result<(), SidebandRunError> {
    for attempt in 0..2 {
        match persistence
            .append_sideband_event_durably(event.clone())
            .await
        {
            Ok(()) => return Ok(()),
            Err(crate::session::persistence::DurableAppendError::AcknowledgementLost(error))
                if attempt == 0 =>
            {
                tracing::warn!(
                    sideband_id = %event.sideband_id,
                    seq = event.seq,
                    %error,
                    "sideband acknowledgement was lost; retrying the exact immutable event"
                );
            }
            Err(error) => return Err(SidebandRunError::Persistence(error.to_string())),
        }
    }
    unreachable!("bounded sideband acknowledgement loop always returns")
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
