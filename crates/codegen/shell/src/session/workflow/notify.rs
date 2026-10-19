use crate::extensions::notification::{
    SessionNotification as GrowSessionNotification, SessionUpdate as GrowSessionUpdate,
    WorkflowAgentInfo, WorkflowPhaseInfo,
};
use crate::session::persistence::PersistenceMsg;

use super::store::WorkflowRunStore;
use super::tracker::WorkflowRunState;

#[derive(Clone)]
pub(crate) struct WorkflowNotifySender {
    session_id: agent_client_protocol::schema::v1::SessionId,
    gateway: acp_transport::AcpAgentGatewaySender,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    budget: sampler::PreviewEventBudget,
    preview: std::sync::Arc<parking_lot::Mutex<Option<(String, u32, bool)>>>,
    store: WorkflowRunStore,
    chat_state: chat_state::ChatStateHandle,
}

impl WorkflowNotifySender {
    pub(crate) fn new(
        session_id: agent_client_protocol::schema::v1::SessionId,
        gateway: acp_transport::AcpAgentGatewaySender,
        persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
        budget: sampler::PreviewEventBudget,
        preview: std::sync::Arc<parking_lot::Mutex<Option<(String, u32, bool)>>>,
        store: WorkflowRunStore,
        chat_state: chat_state::ChatStateHandle,
    ) -> Self {
        Self {
            session_id,
            gateway,
            persistence_tx,
            budget,
            preview,
            store,
            chat_state,
        }
    }

    pub(crate) async fn emit(&self, state: &WorkflowRunState, elapsed_ms: u64, active_agents: u32) {
        if let Err(error) = self.store.persist(state) {
            tracing::warn!(run_id = %state.run_id, %error, "failed to queue workflow manifest");
        }
        self.dispatch(
            build_workflow_updated(state, elapsed_ms, active_agents),
            true,
            Some((
                chat_state::DisplayProjectionKind::WorkflowUpdated,
                state.run_id.clone(),
                state.revision,
            )),
        )
        .await;
    }

    pub(crate) fn emit_ephemeral(
        &self,
        state: &WorkflowRunState,
        elapsed_ms: u64,
        active_agents: u32,
    ) {
        if let Err(error) = self.store.persist(state) {
            tracing::warn!(run_id = %state.run_id, %error, "failed to queue workflow manifest");
        }
        self.dispatch_ephemeral(build_workflow_updated(state, elapsed_ms, active_agents));
    }

    pub(crate) async fn broadcast(
        &self,
        state: &WorkflowRunState,
        elapsed_ms: u64,
        active_agents: u32,
        persist_update: bool,
    ) {
        self.dispatch(
            build_workflow_updated(state, elapsed_ms, active_agents),
            persist_update,
            persist_update.then(|| {
                (
                    chat_state::DisplayProjectionKind::WorkflowUpdated,
                    state.run_id.clone(),
                    state.revision,
                )
            }),
        )
        .await;
    }

    async fn dispatch(
        &self,
        update: GrowSessionUpdate,
        persist: bool,
        projection: Option<(chat_state::DisplayProjectionKind, String, u64)>,
    ) {
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        let notification = GrowSessionNotification {
            session_id: self.session_id.clone(),
            update,
            meta: meta.map(serde_json::Value::Object),
        };
        let Some(prepared) = crate::session::notifications::prepare_auxiliary_grow(
            &self.persistence_tx,
            &self.budget,
            &self.preview,
            notification.clone(),
            persist,
        ) else {
            return;
        };
        if let Some((kind, source_id, source_revision)) = projection
            && let Err(error) =
                crate::session::notifications::record_display_projection_expectation(
                    &self.chat_state,
                    &notification,
                    kind,
                    source_id,
                    source_revision,
                )
                .await
        {
            tracing::error!(%error, "Workflow display projection expectation was not committed; suppressing publication");
            return;
        }
        crate::session::notifications::publish_prepared_auxiliary_grow(
            &self.gateway,
            &self.persistence_tx,
            "grow/session_notification",
            prepared,
        );
    }

    fn dispatch_ephemeral(&self, update: GrowSessionUpdate) {
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        crate::session::notifications::dispatch_auxiliary_grow(
            &self.gateway,
            &self.persistence_tx,
            &self.budget,
            &self.preview,
            "grow/session_notification",
            GrowSessionNotification {
                session_id: self.session_id.clone(),
                update,
                meta: meta.map(serde_json::Value::Object),
            },
            false,
        );
    }
}

pub(crate) fn build_workflow_updated(
    state: &WorkflowRunState,
    elapsed_ms: u64,
    _active_agents: u32,
) -> GrowSessionUpdate {
    let run_complete = state.status == super::tracker::WorkflowRunStatus::Complete;
    let current_idx = state
        .current_phase
        .as_deref()
        .and_then(|cur| state.phases.iter().position(|p| p.title == cur));
    let phases = state
        .phases
        .iter()
        .enumerate()
        .map(|(idx, p)| WorkflowPhaseInfo {
            title: p.title.clone(),
            state: match current_idx {
                Some(cur) if idx < cur => "done",
                Some(cur) if idx == cur && run_complete => "done",
                Some(cur) if idx == cur => "active",
                _ => "pending",
            }
            .to_string(),
        })
        .collect();

    let agents: Vec<WorkflowAgentInfo> = state
        .agents
        .iter()
        .map(|a| WorkflowAgentInfo {
            agent_id: a.agent_id.clone(),
            label: a.label.clone(),
            phase: a.phase.clone(),
            model: a.model.clone(),
            state: a.state.clone(),
            tokens_used: a.tokens_used,
            duration_ms: a.duration_ms,
        })
        .collect();
    let current_agent_label = agents
        .iter()
        .rev()
        .find(|a| a.state == "running")
        .map(|a| a.label.clone());

    let active_agents = agents
        .iter()
        .filter(|agent| agent.state == "running")
        .count() as u32;

    let agents_remaining = state
        .agent_budget
        .map(|total| total.saturating_sub(state.agents_used));

    GrowSessionUpdate::WorkflowUpdated {
        run_id: state.run_id.clone(),
        definition_id: state.definition_id.as_ref().map(ToString::to_string),
        definition_scope: state
            .definition_scope
            .map(|scope| scope.as_str().to_string()),
        definition_hash: state.definition_hash.clone(),
        revision: state.revision,
        name: state.name.clone(),
        objective: state.objective.clone(),
        status: state.status.as_str().to_string(),
        foreground: false,
        phases,
        current_phase: state.current_phase.clone(),
        agent_budget: state.agent_budget,
        agents_used: state.agents_used,
        agents_remaining,
        agent_usage_incomplete: state.agent_usage_incomplete,
        elapsed_ms,
        active_agents,
        current_agent_label,
        agents,
        pause_message: state.pause_message.clone(),
        result_summary: state.result_summary.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::workflow::tracker::WorkflowTracker;

    #[test]
    fn phase_states_derive_from_current() {
        let mut t = WorkflowTracker::default();
        t.start_run(
            "wf_1".into(),
            "demo".into(),
            "obj".into(),
            vec![
                workflow::PhaseMeta {
                    title: "Plan".into(),
                    detail: None,
                },
                workflow::PhaseMeta {
                    title: "Execute".into(),
                    detail: None,
                },
                workflow::PhaseMeta {
                    title: "Verify".into(),
                    detail: None,
                },
            ],
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        t.set_phase("wf_1", "Execute");
        let state = t.get("wf_1").unwrap();
        match build_workflow_updated(&state, 0, 0) {
            GrowSessionUpdate::WorkflowUpdated { phases, .. } => {
                let states: Vec<(String, String)> =
                    phases.into_iter().map(|p| (p.title, p.state)).collect();
                assert_eq!(
                    states,
                    vec![
                        ("Plan".to_string(), "done".to_string()),
                        ("Execute".to_string(), "active".to_string()),
                        ("Verify".to_string(), "pending".to_string()),
                    ]
                );
            }
            _ => panic!("expected WorkflowUpdated"),
        }
    }

    #[test]
    fn completed_run_marks_current_phase_done() {
        let mut t = WorkflowTracker::default();
        t.start_run(
            "wf_2".into(),
            "demo".into(),
            "obj".into(),
            vec![
                workflow::PhaseMeta {
                    title: "Plan".into(),
                    detail: None,
                },
                workflow::PhaseMeta {
                    title: "Verify".into(),
                    detail: None,
                },
            ],
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        t.set_phase("wf_2", "Verify");
        t.apply_outcome(
            "wf_2",
            &workflow::WorkflowOutcome::Completed {
                result: serde_json::json!("done"),
            },
        );
        let state = t.get("wf_2").unwrap();
        match build_workflow_updated(&state, 0, 0) {
            GrowSessionUpdate::WorkflowUpdated { phases, status, .. } => {
                assert_eq!(status, "complete");
                let states: Vec<(String, String)> =
                    phases.into_iter().map(|p| (p.title, p.state)).collect();
                assert_eq!(
                    states,
                    vec![
                        ("Plan".to_string(), "done".to_string()),
                        ("Verify".to_string(), "done".to_string()),
                    ]
                );
            }
            _ => panic!("expected WorkflowUpdated"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ephemeral_and_coalesced_updates_do_not_create_projection_expectations() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let cancellation = tokio_util::sync::CancellationToken::new();
                let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
                let chat_state = chat_state::ChatStateActor::spawn(
                    vec![sampling_types::ConversationItem::system(
                        "test system prompt",
                    )],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".to_string(),
                        model: "test".to_string(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(256_000)
                            .expect("nonzero test context window"),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(chat_state::NullTimelinePersistence),
                    chat_event_tx,
                    cancellation.clone(),
                );
                chat_state
                    .get_sampling_config()
                    .await
                    .expect("chat-state bootstrap must be acknowledged");

                let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let gateway = acp_transport::AcpAgentGatewaySender::new(gateway_tx);
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let store = WorkflowRunStore::new(None, persistence_tx.clone());
                store
                    .register("wf_ephemeral", "()", &serde_json::json!({}))
                    .expect("test source registration needs no session directory");
                let budget = sampler::PreviewEventBudget::default();
                let sender = WorkflowNotifySender::new(
                    agent_client_protocol::schema::v1::SessionId::new("workflow-cache-test"),
                    gateway,
                    persistence_tx,
                    budget.clone(),
                    std::sync::Arc::new(parking_lot::Mutex::new(None)),
                    store,
                    chat_state.clone(),
                );
                let mut tracker = WorkflowTracker::default();
                tracker.start_run(
                    "wf_ephemeral".into(),
                    "test workflow".into(),
                    "exercise non-durable UI updates".into(),
                    vec![workflow::PhaseMeta {
                        title: "Run".into(),
                        detail: None,
                    }],
                    None,
                    None,
                    crate::session::workflow::tracker::test_runtime_route(),
                );
                let state = tracker.get("wf_ephemeral").unwrap();

                sender.emit_ephemeral(&state, 1, 0);
                sender.broadcast(&state, 2, 0, false).await;

                for _ in 0..2 {
                    let message =
                        tokio::time::timeout(std::time::Duration::from_secs(2), gateway_rx.recv())
                            .await
                            .expect("Workflow UI update must be sent")
                            .expect("gateway remains open");
                    let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                        panic!("expected a Grow session notification: {message:?}");
                    };
                    assert_eq!(args.request.method.as_ref(), "grow/session_notification");
                    let _ = args.response_tx.send(Ok(()));
                }

                // A durable update rejected before it can reserve publication
                // capacity must not leave a phantom cache expectation either.
                let _all_credits =
                    tokio::time::timeout(std::time::Duration::from_secs(2), budget.acquire(4096))
                        .await
                        .expect("completed preview sends release their reservations")
                        .expect("preview budget remains open");
                sender.broadcast(&state, 3, 0, true).await;
                assert!(gateway_rx.try_recv().is_err());

                let timeline = chat_state
                    .timeline_events()
                    .await
                    .expect("Timeline remains available");
                assert!(!timeline.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::DisplayProjectionExpected(_)
                )));
                while let Ok(message) = persistence_rx.try_recv() {
                    assert!(
                        !matches!(message, PersistenceMsg::AuxiliaryGrow { .. }),
                        "ephemeral/coalesced updates must not enter the display cache"
                    );
                }
                cancellation.cancel();
            })
            .await;
    }
}
