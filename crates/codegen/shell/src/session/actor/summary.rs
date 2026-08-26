//! Session-title sideband and adopted-title notifications.

use crate::sampling::SamplingClient;
use crate::session::info::Info;
use crate::session::{SessionActor, sideband::SidebandSource};
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use agent_client_protocol as acp;

pub(crate) struct SessionTitleRoute {
    client: SamplingClient,
    model: String,
}

impl SessionTitleRoute {
    pub(crate) fn new(client: SamplingClient, model: String) -> Self {
        Self { client, model }
    }
}

impl SessionActor {
    /// Claim the one-shot title route after the first real user message is
    /// durable, freeze that exact Timeline event, then run the provider call
    /// independently on the session LocalSet.
    pub(crate) async fn schedule_session_title(self: &std::sync::Arc<Self>, user_text: String) {
        if user_text.trim().is_empty() {
            return;
        }
        let Some(route) = self.session_title_route.borrow_mut().take() else {
            return;
        };
        let Some(materialized) = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await
        else {
            self.session_title_route.replace(Some(route));
            tracing::warn!("session title: failed to freeze Timeline input");
            return;
        };
        let input_ref = chat_state::TimelineRangeRef {
            timeline_id: materialized.input_ref.timeline_id,
            first_seq: materialized.input_ref.last_seq,
            last_seq: materialized.input_ref.last_seq,
        };
        let session = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            session
                .generate_session_title(route, user_text, input_ref)
                .await;
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
                self.session_title_route.replace(Some(route));
                return;
            }
        };
        if let Err(error) = sideband
            .attempt_all_sources(&request, route.client.api_backend(), None)
            .await
        {
            tracing::warn!(%error, "session title: failed to commit Sideband attempt");
            self.session_title_route.replace(Some(route));
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
                        self.session_title_route.replace(Some(route));
                        return;
                    }
                };
                self.persist_title_fallback(&user_text, terminal_ref).await;
                return;
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "session title: provider admission failed");
                self.session_title_route.replace(Some(route));
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
                        self.session_title_route.replace(Some(route));
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
                self.session_title_route.replace(Some(route));
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
                        self.session_title_route.replace(Some(route));
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
                self.session_title_route.replace(Some(route));
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
