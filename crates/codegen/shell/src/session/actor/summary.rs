//! Session-title sideband and adopted-title notifications.

use crate::sampling::SamplingClient;
use crate::session::info::Info;
use crate::session::{SessionActor, sideband::SidebandSource};
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use acp_transport::protocol as acp;

pub(crate) struct SessionTitleRoute {
    client: SamplingClient,
    model: String,
}

impl SessionTitleRoute {
    pub(crate) fn new(client: SamplingClient, model: String) -> Self {
        Self { client, model }
    }
}

/// The empty slot while a worker owns the route must not erase revocation.
pub(crate) enum SessionTitleRouteState {
    Ready(SessionTitleRoute),
    Claimed,
    Closed,
}

impl From<Option<SessionTitleRoute>> for SessionTitleRouteState {
    fn from(route: Option<SessionTitleRoute>) -> Self {
        route.map(Self::Ready).unwrap_or(Self::Closed)
    }
}

impl SessionTitleRouteState {
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    fn claim(&mut self) -> Option<SessionTitleRoute> {
        if !self.is_ready() {
            return None;
        }
        match std::mem::replace(self, Self::Claimed) {
            Self::Ready(route) => Some(route),
            _ => unreachable!("readiness checked without yielding"),
        }
    }

    fn restore(&mut self, route: SessionTitleRoute) {
        if matches!(self, Self::Claimed) {
            *self = Self::Ready(route);
        }
    }

    pub(crate) fn revoke(&mut self) {
        *self = Self::Closed;
    }

    fn finish(&mut self) {
        if matches!(self, Self::Claimed) {
            *self = Self::Closed;
        }
    }
}

fn title_input_source(
    materialized: &chat_state::TimelineMaterialization,
    prompt_index: usize,
) -> Option<chat_state::SurfaceId> {
    if materialized.surface.len() != materialized.surface_ids.len() {
        return None;
    }
    let mut sources = materialized
        .surface
        .iter()
        .zip(&materialized.surface_ids)
        .filter_map(|(item, source)| match item {
            sampling_types::ConversationItem::User(user)
                if user.synthetic_reason.is_none() && user.prompt_index == Some(prompt_index) =>
            {
                Some(*source)
            }
            _ => None,
        });
    let source = sources.next()?;
    sources.next().is_none().then_some(source)
}

impl SessionActor {
    /// Claim the one-shot title route after the first real user message is
    /// durable, freeze that exact Timeline event, then run the provider call
    /// independently on the session LocalSet.
    pub(crate) async fn schedule_session_title_for_prompt(
        self: &std::sync::Arc<Self>,
        user_text: String,
        prompt_index: usize,
    ) {
        if user_text.trim().is_empty() {
            return;
        }
        if !self.session_title_route.borrow().is_ready() {
            return;
        }
        let Some(materialized) = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await
        else {
            tracing::warn!("session title: failed to freeze Timeline input");
            return;
        };
        let Some(source) = title_input_source(&materialized, prompt_index) else {
            tracing::warn!(
                prompt_index,
                "session title: admitted user source is missing or ambiguous"
            );
            return;
        };
        self.schedule_session_title(user_text, source.event).await;
    }

    pub(crate) async fn schedule_session_title(
        self: &std::sync::Arc<Self>,
        user_text: String,
        input_event: chat_state::EventSeq,
    ) {
        if user_text.trim().is_empty() {
            return;
        }
        let Some(route) = self.session_title_route.borrow_mut().claim() else {
            return;
        };
        let input_ref = chat_state::TimelineRangeRef {
            timeline_id: self.session_info.id.to_string(),
            first_seq: input_event.get(),
            last_seq: input_event.get(),
        };
        let Some(activity) = self.session_activities.try_start("session_title") else {
            self.session_title_route.borrow_mut().finish();
            return;
        };
        let session = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            let _activity = activity;
            session
                .generate_session_title(route, user_text, input_ref)
                .await;
            session.session_title_route.borrow_mut().finish();
        });
    }

    async fn generate_session_title(
        &self,
        route: SessionTitleRoute,
        user_text: String,
        input_ref: chat_state::TimelineRangeRef,
    ) {
        use crate::session::actor::sideband::sideband_finish;
        use crate::session::helpers::session_title;

        let request = session_title::build_session_title_request(
            &user_text,
            &route.model,
            route.client.api_backend(),
        );
        let mut sideband = match self
            .begin_sideband(
                chat_state::SidebandPurpose::SessionTitle,
                session_title::SESSION_TITLE_PROMPT.into(),
                SidebandSource::Frozen(vec![input_ref]),
                chat_state::SidebandBudgetPolicy::for_request(&request, 1),
                chat_state::SidebandRoute {
                    model: route.model.clone(),
                    backend: route.client.api_backend(),
                },
                Some(session_title::session_title_output_schema()),
            )
            .await
        {
            Ok(sideband) => sideband,
            Err(error) => {
                tracing::warn!(%error, "session title: failed to start Sideband");
                self.session_title_route.borrow_mut().restore(route);
                return;
            }
        };
        if let Err(error) = sideband
            .attempt_all_sources(&request, route.client.api_backend(), None)
            .await
        {
            tracing::warn!(%error, "session title: failed to commit Sideband attempt");
            self.session_title_route.borrow_mut().restore(route);
            return;
        }
        let response = match tokio::time::timeout(
            session_title::SESSION_TITLE_TIMEOUT,
            sideband.run_provider(route.client.conversation_collect(request)),
        )
        .await
        {
            Ok(Ok(Ok(response))) => response,
            Ok(Ok(Err(error))) => {
                let terminal_ref = match sideband
                    .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                    .await
                {
                    Ok(reference) => reference,
                    Err(record_error) => {
                        tracing::warn!(%record_error, "session title: failed to commit provider failure");
                        self.session_title_route.borrow_mut().restore(route);
                        return;
                    }
                };
                self.persist_title_fallback(&user_text, terminal_ref).await;
                return;
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "session title: provider admission failed");
                self.session_title_route.borrow_mut().restore(route);
                return;
            }
            Err(_) => {
                let terminal_ref = match sideband
                    .fail(
                        chat_state::SidebandOutcome::Cancelled,
                        "session title generation timed out",
                    )
                    .await
                {
                    Ok(reference) => reference,
                    Err(record_error) => {
                        tracing::warn!(%record_error, "session title: failed to commit timeout");
                        self.session_title_route.borrow_mut().restore(route);
                        return;
                    }
                };
                self.persist_title_fallback(&user_text, terminal_ref).await;
                return;
            }
        };
        let usage = match self
            .settle_sideband_response_usage(&mut sideband, &response)
            .await
        {
            Ok(usage) => usage,
            Err(error) => {
                tracing::warn!(%error, "session title: failed to settle provider usage");
                self.session_title_route.borrow_mut().restore(route);
                return;
            }
        };

        let raw_output = response.assistant_text();
        let title = match session_title::parse_session_title_output(&raw_output) {
            Ok(title) => title,
            Err(error) => {
                let terminal_ref = match sideband
                    .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                    .await
                {
                    Ok(reference) => reference,
                    Err(record_error) => {
                        tracing::warn!(%record_error, "session title: failed to commit validation failure");
                        self.session_title_route.borrow_mut().restore(route);
                        return;
                    }
                };
                self.persist_title_fallback(&user_text, terminal_ref).await;
                return;
            }
        };
        let result_ref = match sideband
            .complete(
                raw_output,
                Some(serde_json::json!({ "session_title": title.clone() })),
                usage,
                sideband_finish(&response),
                Vec::new(),
            )
            .await
        {
            Ok(reference) => reference,
            Err(error) => {
                tracing::warn!(%error, "session title: failed to commit Sideband result");
                self.session_title_route.borrow_mut().restore(route);
                return;
            }
        };
        if let Err(error) = self
            .commit_session_title(
                title,
                chat_state::SessionTitleSource::Generated {
                    sideband_id: result_ref.timeline_id,
                    result_seq: result_ref.first_seq,
                },
            )
            .await
        {
            tracing::warn!(%error, "session title: canonical title event was not adopted");
        }
    }

    async fn persist_title_fallback(
        &self,
        user_text: &str,
        terminal_ref: chat_state::TimelineRangeRef,
    ) {
        let title =
            crate::session::helpers::session_title::title_fallback_from_user_text(user_text);
        if let Err(error) = self
            .commit_session_title(
                title,
                chat_state::SessionTitleSource::Fallback {
                    sideband_id: terminal_ref.timeline_id,
                    terminal_seq: terminal_ref.first_seq,
                },
            )
            .await
        {
            tracing::warn!(%error, "session title: fallback title event was not adopted");
        }
    }

    pub(crate) async fn commit_session_title(
        &self,
        title: String,
        source: chat_state::SessionTitleSource,
    ) -> Result<chat_state::TimelineEvent, String> {
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::SessionTitle(
                chat_state::SessionTitleEvent { title, source },
            ))
            .await
            .map_err(|error| error.to_string())
    }
}

/// Notify the client that a session summary is available.
pub(crate) fn notify_client(
    gateway: &Option<GatewaySender>,
    info: &Info,
    event_seq: u64,
    title: &chat_state::SessionTitleEvent,
) {
    let Some(gateway) = gateway else {
        return;
    };
    gateway.forward_fire_and_forget(session_info_update(info.id.clone(), event_seq, title));
}

pub(crate) fn session_info_update(
    session_id: acp::SessionId,
    event_seq: u64,
    title: &chat_state::SessionTitleEvent,
) -> acp::SessionNotification {
    // `updatedAt` is omitted, not refreshed: renaming is not activity, and
    // `session/list` sorts on `last_active_at`, which a title write never moves.
    acp::SessionNotification::new(
        session_id,
        acp::SessionUpdate::SessionInfoUpdate(
            acp::SessionInfoUpdate::new()
                .title(title.title.clone())
                .meta(
                    serde_json::json!({
                        "grow/titleEventSeq": event_seq,
                        "grow/titleSource": match &title.source {
                            chat_state::SessionTitleSource::User => "user",
                            chat_state::SessionTitleSource::Generated { .. } => "generated",
                            chat_state::SessionTitleSource::Fallback { .. } => "fallback",
                        },
                    })
                    .as_object()
                    .cloned(),
                ),
        ),
    )
}

#[cfg(test)]
mod title_source_tests {
    use super::*;
    use sampling_types::ConversationItem;

    fn materialized(items: Vec<ConversationItem>) -> chat_state::TimelineMaterialization {
        let mut timeline = chat_state::Timeline::default();
        for item in items {
            timeline
                .append(item, chat_state::MessageCause::User)
                .unwrap();
        }
        // A non-message fact can become the tail after the admitted input.
        timeline
            .record(chat_state::TimelineEventKind::SessionTitle(
                chat_state::SessionTitleEvent {
                    title: "manual".into(),
                    source: chat_state::SessionTitleSource::User,
                },
            ))
            .unwrap();
        chat_state::TimelineMaterialization {
            input_ref: chat_state::TimelineRangeRef {
                timeline_id: "session".into(),
                first_seq: 0,
                last_seq: timeline.events().last().unwrap().seq.get(),
            },
            surface_revision: timeline.surface_revision(),
            surface: timeline.surface().to_vec(),
            surface_ids: timeline.surface_ids().to_vec(),
            direct_user_inputs: vec![],
            permission_context: vec![],
            active_control_contexts: Default::default(),
        }
    }

    fn user(text: &str, index: usize) -> ConversationItem {
        let mut item = ConversationItem::user(text);
        item.set_prompt_index(index);
        item
    }

    #[test]
    fn title_source_stays_on_user_before_later_events() {
        let mut notification = ConversationItem::notification_drain("finished background task");
        notification.set_prompt_index(2);
        let snapshot = materialized(vec![
            user("old", 1),
            user("title objective", 2),
            notification,
        ]);
        let source = title_input_source(&snapshot, 2).unwrap();
        assert_eq!(source, snapshot.surface_ids[1]);
        assert_ne!(source.event.get(), snapshot.input_ref.last_seq);
        assert_ne!(source, snapshot.surface_ids[2]);
    }

    #[test]
    fn title_source_requires_unique_admitted_identity() {
        let snapshot = materialized(vec![user("same", 2), user("same", 2)]);
        assert!(title_input_source(&snapshot, 2).is_none());
        assert!(title_input_source(&snapshot, 3).is_none());
        let mut notification = ConversationItem::notification_drain("same");
        notification.set_prompt_index(2);
        assert!(title_input_source(&materialized(vec![notification]), 2).is_none());
    }
}

#[cfg(test)]
mod title_route_tests {
    use super::*;

    fn ready() -> SessionTitleRouteState {
        let client = SamplingClient::new(sampler::SamplerConfig {
            api_key: Some("test-key".into()),
            base_url: "http://127.0.0.1:1".into(),
            model: "test-model".into(),
            ..Default::default()
        })
        .unwrap();
        Some(SessionTitleRoute::new(client, "test-model".into())).into()
    }

    #[tokio::test]
    async fn revoked_claim_cannot_restore_generation() {
        let mut state = ready();
        let owned = state.claim().unwrap();
        assert!(state.claim().is_none());
        state.revoke();
        state.restore(owned);
        state.finish();
        assert!(matches!(state, SessionTitleRouteState::Closed));
        assert!(state.claim().is_none());
    }

    #[tokio::test]
    async fn valid_failed_claim_can_retry_once() {
        let mut state = ready();
        let owned = state.claim().unwrap();
        state.restore(owned);
        state.finish();
        assert!(
            state.is_ready(),
            "worker finalization preserves restored retry"
        );
        let _retry = state.claim().unwrap();
        assert!(state.claim().is_none());
        state.finish();
        assert!(state.claim().is_none(), "completed claim is one-shot");
    }

    #[tokio::test]
    async fn revocation_before_claim_or_after_restore_stays_closed() {
        let mut state = ready();
        state.revoke();
        assert!(state.claim().is_none());
        let mut state = ready();
        let owned = state.claim().unwrap();
        state.restore(owned);
        state.revoke();
        state.finish();
        assert!(state.claim().is_none());
        let mut absent = SessionTitleRouteState::from(None);
        assert!(absent.claim().is_none());
    }
}
