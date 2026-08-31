use super::*;

impl MvpAgent {
    pub(super) async fn ensure_coordination_started(&self) -> bool {
        match self.coordination.ensure_started().await {
            Ok(()) => {
                self.publish_coordination_snapshot().await;
                self.start_coordination_publisher();
                true
            }
            Err(error) => {
                tracing::warn!(error = %error, "local agent coordination is unavailable");
                false
            }
        }
    }

    fn start_coordination_publisher(&self) {
        if self.coordination_publisher_started.replace(true) {
            return;
        }
        let mut inquiry_rx = self
            .coordination
            .take_inquiry_receiver()
            .expect("coordination inquiry receiver is taken once with the publisher");
        let inquiry_agent = LocalRef::new(self);
        tokio::task::spawn_local(async move {
            while let Some(inquiry) = inquiry_rx.recv().await {
                let target_id = acp::SessionId::new(inquiry.target_session_id.clone());
                let target = inquiry_agent
                    .get()
                    .sessions
                    .borrow()
                    .get(&target_id)
                    .cloned();
                if let Some(target) = target {
                    if let Err(error) = target
                        .cmd_tx
                        .send(SessionCommand::RunCoordinationInquiry { inquiry })
                    {
                        let SessionCommand::RunCoordinationInquiry { inquiry } = error.0 else {
                            unreachable!("coordination dispatcher sent one command variant");
                        };
                        reject_unavailable(inquiry);
                    }
                } else {
                    reject_unavailable(inquiry);
                }
            }
        });
        let agent_ref = LocalRef::new(self);
        tokio::task::spawn_local(async move {
            loop {
                tokio::select! {
                    _ = agent_ref.get().coordination.cancelled() => break,
                    _ = tokio::time::sleep(crate::coordination::HEARTBEAT_INTERVAL) => {
                        agent_ref.get().publish_coordination_snapshot().await;
                    }
                }
            }
        });
    }

    pub(super) async fn publish_coordination_snapshot(&self) {
        let sessions: Vec<_> = self
            .sessions
            .borrow()
            .iter()
            .map(|(id, handle)| (id.clone(), handle.clone()))
            .collect();
        let mut snapshots = Vec::with_capacity(sessions.len());
        for (id, handle) in sessions {
            if handle.cmd_tx.is_closed() {
                continue;
            }
            let active_subagents = self.active_subagent_count(id.0.as_ref()).await;
            snapshots.push(crate::coordination::LocalSessionSnapshot {
                session_id: id.0.to_string(),
                canonical_cwd: crate::coordination::canonical_cwd(std::path::Path::new(
                    &handle.info.cwd,
                )),
                main_agent: handle.agent_profile.name(),
                activity: self.resident_activity(&id),
                subagents: crate::coordination::SubagentStats {
                    active: active_subagents,
                },
            });
        }
        self.coordination.publish_sessions(snapshots);
    }

    async fn active_subagent_count(&self, parent_session_id: &str) -> usize {
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentListActiveRequest,
        };

        let (respond_to, response) = tokio::sync::oneshot::channel();
        if self
            .subagent_event_tx
            .send(SubagentEvent::ListActive(SubagentListActiveRequest {
                parent_session_id: parent_session_id.to_owned(),
                respond_to,
            }))
            .is_err()
        {
            return 0;
        }
        tokio::time::timeout(std::time::Duration::from_millis(500), response)
            .await
            .ok()
            .and_then(Result::ok)
            .map_or(0, |subagents| subagents.len())
    }

    pub(crate) async fn list_coordination_sessions(
        &self,
        source_session_id: &str,
    ) -> Result<Vec<crate::coordination::DiscoveredSession>, String> {
        self.coordination_backend(source_session_id)?
            .list_active_sessions()
            .await
    }

    pub(crate) fn validate_coordination_source(
        &self,
        source_session_id: &str,
    ) -> Result<(), String> {
        self.require_coordination_source(source_session_id)
            .map(|_| ())
    }

    pub(crate) async fn ask_coordination_session(
        &self,
        inquiry_id: String,
        source_session_id: String,
        target_session_id: String,
        question: String,
        progress: Option<tokio::sync::mpsc::UnboundedSender<crate::coordination::InquiryPhase>>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<crate::coordination::InquiryOutcome, String> {
        self.coordination_backend(&source_session_id)?
            .ask_with_id(
                inquiry_id,
                target_session_id,
                question,
                progress,
                cancellation,
            )
            .await
    }

    pub(crate) async fn cancel_coordination_session(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
        target_session_id: &str,
    ) -> Result<bool, String> {
        self.coordination_backend(source_session_id)?
            .cancel_session(inquiry_id, target_session_id)
            .await
    }

    pub(crate) fn coordination_backend_resource(
        &self,
        source: SessionHandle,
    ) -> tools::implementations::grow_build::coordination::CoordinationBackendResource {
        tools::implementations::grow_build::coordination::CoordinationBackendResource(
            std::sync::Arc::new(SessionCoordinationBackend {
                coordination: self.coordination.handle(),
                source_session_id: source.info.id.0.to_string(),
                source,
            }),
        )
    }

    fn coordination_backend(
        &self,
        source_session_id: &str,
    ) -> Result<SessionCoordinationBackend, String> {
        let source = self.require_coordination_source(source_session_id)?;
        Ok(SessionCoordinationBackend {
            coordination: self.coordination.handle(),
            source_session_id: source_session_id.to_owned(),
            source,
        })
    }

    fn require_coordination_source(
        &self,
        source_session_id: &str,
    ) -> Result<SessionHandle, String> {
        self.sessions
            .borrow()
            .get(&acp::SessionId::new(source_session_id))
            .cloned()
            .filter(|handle| !handle.cmd_tx.is_closed())
            .ok_or_else(|| "source session is not owned by this ACP connection".to_owned())
    }
}

#[derive(Clone)]
struct SessionCoordinationBackend {
    coordination: crate::coordination::CoordinationHandle,
    source_session_id: String,
    source: SessionHandle,
}

impl SessionCoordinationBackend {
    async fn list_active_sessions(
        &self,
    ) -> Result<Vec<crate::coordination::DiscoveredSession>, String> {
        self.coordination
            .list_active_sessions(&self.source_session_id)
            .await
    }

    async fn ask_with_id(
        &self,
        inquiry_id: String,
        target_session_id: String,
        question: String,
        progress: Option<tokio::sync::mpsc::UnboundedSender<crate::coordination::InquiryPhase>>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<crate::coordination::InquiryOutcome, String> {
        record_coordination_notice(
            &self.source,
            crate::extensions::notification::UiNotice {
                correlation_id: inquiry_id.clone(),
                category: crate::extensions::notification::UiNoticeCategory::Coordination,
                subject: Some("outgoing inquiry".to_owned()),
                description: Some("Question sent to another local Grow session".to_owned()),
                message: format!("Asking session {target_session_id}"),
                tone: crate::extensions::notification::UiNoticeTone::Info,
                details: Some(format!(
                    "Target session: {target_session_id}\n\nQuestion:\n{question}"
                )),
            },
        )
        .await?;

        let coordination = self.coordination.clone();
        let task_source = self.source.clone();
        let task_source_session_id = self.source_session_id.clone();
        let task_inquiry_id = inquiry_id.clone();
        let task_target_session_id = target_session_id.clone();
        let task_question = question.clone();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let result = coordination
                .ask_session(
                    &task_inquiry_id,
                    &task_source_session_id,
                    &task_target_session_id,
                    &task_question,
                    progress,
                    task_cancellation,
                )
                .await;
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => crate::coordination::InquiryOutcome::terminal(
                    &task_inquiry_id,
                    crate::coordination::InquiryStatus::Failed,
                    error,
                ),
            };
            record_coordination_notice(
                &task_source,
                source_terminal_notice(&task_target_session_id, &outcome),
            )
            .await?;
            Ok::<_, String>(outcome)
        });
        let mut cancel_on_drop = CancelCoordinationOnDrop::new(cancellation);
        let result = task
            .await
            .map_err(|error| format!("coordination inquiry task failed: {error}"))?;
        cancel_on_drop.disarm();
        result
    }

    async fn cancel_session(
        &self,
        inquiry_id: &str,
        target_session_id: &str,
    ) -> Result<bool, String> {
        self.coordination
            .cancel_session(inquiry_id, &self.source_session_id, target_session_id)
            .await
    }
}

#[async_trait::async_trait]
impl tools::implementations::grow_build::coordination::CoordinationBackend
    for SessionCoordinationBackend
{
    async fn list_active_sessions(
        &self,
    ) -> Result<Vec<tools::implementations::grow_build::coordination::ActiveSession>, String> {
        Ok(self
            .list_active_sessions()
            .await?
            .into_iter()
            .map(
                |session| tools::implementations::grow_build::coordination::ActiveSession {
                    session_id: session.session_id,
                    canonical_cwd: session.canonical_cwd,
                    main_agent: session.main_agent,
                    activity: enum_label(session.activity),
                    active_subagents: session.subagents.active,
                    started_at: session.started_at,
                    process_started_at: session.process_started_at,
                    last_heartbeat: session.last_heartbeat,
                },
            )
            .collect())
    }

    async fn ask_session(
        &self,
        target_session_id: String,
        question: String,
        progress: tokio::sync::mpsc::UnboundedSender<String>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<tools::implementations::grow_build::coordination::CoordinationInquiryResult, String>
    {
        let inquiry_id = uuid::Uuid::now_v7().to_string();
        let (typed_progress, mut typed_phases) = tokio::sync::mpsc::unbounded_channel();
        let progress_task = tokio::spawn(async move {
            while let Some(phase) = typed_phases.recv().await {
                let _ = progress.send(enum_label(phase));
            }
        });
        let outcome = self
            .ask_with_id(
                inquiry_id.clone(),
                target_session_id,
                question,
                Some(typed_progress),
                cancellation,
            )
            .await
            .unwrap_or_else(|error| {
                crate::coordination::InquiryOutcome::terminal(
                    inquiry_id,
                    crate::coordination::InquiryStatus::Failed,
                    error,
                )
            });
        let _ = progress_task.await;
        Ok(
            tools::implementations::grow_build::coordination::CoordinationInquiryResult {
                inquiry_id: outcome.inquiry_id,
                status: enum_label(outcome.status),
                answer: outcome.answer,
                error: outcome.error,
            },
        )
    }
}

fn enum_label<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

struct CancelCoordinationOnDrop {
    cancellation: Option<tokio_util::sync::CancellationToken>,
}

impl CancelCoordinationOnDrop {
    fn new(cancellation: tokio_util::sync::CancellationToken) -> Self {
        Self {
            cancellation: Some(cancellation),
        }
    }

    fn disarm(&mut self) {
        self.cancellation = None;
    }
}

impl Drop for CancelCoordinationOnDrop {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.cancel();
        }
    }
}

async fn record_coordination_notice(
    session: &SessionHandle,
    notice: crate::extensions::notification::UiNotice,
) -> Result<(), String> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    session
        .cmd_tx
        .send(SessionCommand::RecordCoordinationNotice { notice, respond_to })
        .map_err(|_| "source session is unavailable".to_owned())?;
    tokio::time::timeout(std::time::Duration::from_secs(30), response)
        .await
        .map_err(|_| "coordination audit persistence timed out".to_owned())?
        .map_err(|_| "source session closed before persisting coordination audit".to_owned())?
}

fn source_terminal_notice(
    target_session_id: &str,
    outcome: &crate::coordination::InquiryOutcome,
) -> crate::extensions::notification::UiNotice {
    let status = serde_json::to_value(outcome.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "failed".to_owned());
    let details = match (&outcome.answer, &outcome.error) {
        (Some(answer), _) => Some(format!(
            "Target session: {target_session_id}\nStatus: {status}\n\nAnswer:\n{answer}"
        )),
        (_, Some(error)) => Some(format!(
            "Target session: {target_session_id}\nStatus: {status}\nError: {error}"
        )),
        _ => Some(format!(
            "Target session: {target_session_id}\nStatus: {status}"
        )),
    };
    crate::extensions::notification::UiNotice {
        correlation_id: outcome.inquiry_id.clone(),
        category: crate::extensions::notification::UiNoticeCategory::Coordination,
        subject: Some("outgoing inquiry completed".to_owned()),
        description: Some("Local coordination inquiry terminal state".to_owned()),
        message: format!("Inquiry to session {target_session_id} finished: {status}"),
        tone: if outcome.status == crate::coordination::InquiryStatus::Answered {
            crate::extensions::notification::UiNoticeTone::Success
        } else if outcome.status == crate::coordination::InquiryStatus::Failed {
            crate::extensions::notification::UiNoticeTone::Error
        } else {
            crate::extensions::notification::UiNoticeTone::Warning
        },
        details,
    }
}

fn reject_unavailable(inquiry: crate::coordination::InboundInquiry) {
    let _ = inquiry
        .respond_to
        .send(crate::coordination::InquiryOutcome::terminal(
            inquiry.inquiry_id,
            crate::coordination::InquiryStatus::Unavailable,
            "target session is unavailable",
        ));
}
