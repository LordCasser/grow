use super::*;
use crate::session::actor::tests::support::build_actor;
use serde_json::json;
use std::time::Duration;
use tools::implementations::grow_build::task::coordinator::{
    ChildCompletion, ChildRunRequest, ChildRunner, CoordinatorConfig, LocalBoxFuture,
    SubagentCoordinator,
};
use tools::implementations::grow_build::task::types::{
    SubagentEvent, SubagentOwner, SubagentValidateTypeOutcome,
};

fn spawn() -> chat_state::SubagentSpawnEvent {
    chat_state::SubagentSpawnEvent {
        subagent_id: "restore-child".into(),
        child_session_id: "restore-child".into(),
        origin_tool_call_id: None,
        security_parent_session_id: "test-parent".into(),
        parent_step_id: Some("canonical-step".into()),
        subagent_type: "general-purpose".into(),
        description: "restore the admitted execution".into(),
        prompt: "old prompt mentions stale-source-parent and stale-source-step".into(),
        execution_recipe: json!({}),
        context_source: chat_state::SubagentContextSource::New,
        source_ref: None,
        context_normalized: false,
        resumed_from: Some("old-source".into()),
        parent_prompt_id: None,
        capability_mode: Some("all".into()),
        birth_mcp_servers: Some(Default::default()),
        permission_mode: None,
        effective_permission_mode: None,
        workflow_run_id: None,
        goal_id: None,
        goal_definition_revision: None,
        surface_completion: false,
        child_cwd: "/tmp".into(),
        worktree_path: None,
        effective_model_id: "model".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "model",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_spawn_survives_cold_decode_and_ignores_old_resume_prompt_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (parent, _gateway) = build_actor().await;
            let source = spawn();
            parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Spawned(Box::new(source)),
                ))
                .await
                .unwrap();
            let events = parent.chat_state_handle.timeline_events().await.unwrap();
            let encoded = serde_json::to_vec(&events).unwrap();
            let decoded = serde_json::from_slice(&encoded).unwrap();
            let cold = chat_state::Timeline::from_events(decoded).unwrap();
            let canonical = cold
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) => Some(spawn),
                    _ => None,
                })
                .unwrap();
            let binding = parent_plan_binding_from_spawn(canonical).unwrap();
            assert_eq!(binding.parent_session_id, "test-parent");
            assert_eq!(binding.task_id, "restore-child");
            assert_eq!(binding.parent_step_id, "canonical-step");
            let mut resumed = canonical.as_ref().clone();
            resumed.subagent_id = "new-execution".into();
            resumed.security_parent_session_id = "new-direct-parent".into();
            resumed.parent_step_id = Some("new-current-step".into());
            let new = parent_plan_binding_from_spawn(&resumed).unwrap();
            assert_eq!(new.parent_session_id, "new-direct-parent");
            assert_eq!(new.task_id, "new-execution");
            assert_eq!(new.parent_step_id, "new-current-step");
            resumed.parent_step_id = None;
            assert_eq!(parent_plan_binding_from_spawn(&resumed), None);
        })
        .await;
}

struct RestoreRunner {
    context: std::cell::RefCell<Option<SubagentSpawnContext>>,
    gateway: GatewaySender,
}

impl ChildRunner for RestoreRunner {
    type Control = ShellChildRuntime;
    type CompletionData = ShellCompletionData;
    type RunFuture = LocalBoxFuture<ChildRunOutcome<ShellCompletionData>>;
    type ValidateFuture = std::future::Ready<SubagentValidateTypeOutcome>;

    fn run(&self, run: ChildRunRequest<ShellChildRuntime>) -> Self::RunFuture {
        let context = self.context.borrow_mut().take().unwrap();
        let gateway = self.gateway.clone();
        Box::pin(async move {
            // Enter the actual Shell handler. A mismatched restored step must
            // fail before reading child storage, hooks, provider or worktree setup.
            run_shell_child_with_restore(
                run.request,
                context,
                run.cancellation,
                run.reporter,
                &gateway,
                run.restore,
            )
            .await
        })
    }
    fn validate_type(&self, _: String, _: String) -> Self::ValidateFuture {
        std::future::ready(SubagentValidateTypeOutcome::Ok)
    }
    fn on_completed(&self, _: ChildCompletion<ShellCompletionData>) {
        panic!("a rejected restore is not a terminal child result");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn actual_restore_handler_rejects_a_request_that_changes_the_canonical_parent_step() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (parent, mut parent_gateway) = build_actor().await;
            parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Spawned(Box::new(spawn())),
                ))
                .await
                .unwrap();
            let before = parent.chat_state_handle.timeline_events().await.unwrap();
            let mut context = crate::test_support::lsp_runtime::ctx_with_toggle(Default::default());
            context.parent_chat_state = Some(parent.chat_state_handle.clone());
            let (gateway, mut gateway_rx) =
                crate::test_support::lsp_runtime::test_gateway_with_receiver();
            let (events, receiver) = mpsc::unbounded_channel();
            let coordinator = tokio::task::spawn_local(
                SubagentCoordinator::new(
                    receiver,
                    RestoreRunner {
                        context: std::cell::RefCell::new(Some(context)),
                        gateway,
                    },
                    CoordinatorConfig::default(),
                )
                .run(),
            );
            let (respond_to, response) = oneshot::channel();
            events
                .send(SubagentEvent::AdoptSuspended {
                    request: Box::new(SubagentRequest {
                        id: "restore-child".into(),
                        origin_tool_call_id: None,
                        prompt: "do not adopt a source's previous parent step".into(),
                        description: "restore".into(),
                        subagent_type: "general-purpose".into(),
                        parent_session_id: "test-parent".into(),
                        parent_step_id: Some("changed-step".into()),
                        parent_prompt_id: None,
                        resume_from: None,
                        cwd: None,
                        runtime_overrides: Default::default(),
                        run_in_background: true,
                        surface_completion: false,
                        await_to_completion: false,
                        fork_context: false,
                        owner: SubagentOwner::Task,
                        goal_context: None,
                        cancel_token: CancellationToken::new(),
                    }),
                    security_parent_session_id: "test-parent".into(),
                    checkpoint: ChildCheckpoint {
                        session_id: "restore-child".into(),
                        operation_id: "restore-operation".into(),
                        checkpoint_seq: 1,
                    },
                    respond_to,
                })
                .unwrap();
            let result = tokio::time::timeout(Duration::from_secs(2), response)
                .await
                .unwrap()
                .unwrap();
            assert!(
                result
                    .unwrap_err()
                    .contains("original parent Spawn planning binding changed")
            );
            assert_eq!(
                serde_json::to_value(parent.chat_state_handle.timeline_events().await.unwrap())
                    .unwrap(),
                serde_json::to_value(before).unwrap()
            );
            assert!(gateway_rx.try_recv().is_err());
            assert!(parent_gateway.try_recv().is_err());
            coordinator.abort();
            assert!(coordinator.await.unwrap_err().is_cancelled());
        })
        .await;
}
